//! Native OpenRig mission control.
use super::Panel;
use crate::theme::Theme;
use gpui::{Context, FontWeight, MouseButton, ScrollHandle, Task, Window, div, prelude::*, px};
use jcode_desktop_openrig::{Snapshot, default_base_url, fetch_snapshot};
use std::{collections::HashSet, time::Duration};

pub(crate) const SESSION_ID: &str = "openrig://mission-control";

pub(super) struct State {
    snapshot: Snapshot,
    section: usize,
    selected: usize,
    filter: String,
    command: Option<String>,
    explorer: bool,
    expanded: HashSet<String>,
    scroll: ScrollHandle,
    _poll: Option<Task<()>>,
}
impl State {
    pub(super) fn fixture() -> Self {
        Self {
            snapshot: Snapshot::fixture(),
            section: 0,
            selected: 0,
            filter: String::new(),
            command: None,
            explorer: true,
            expanded: HashSet::new(),
            scroll: ScrollHandle::new(),
            _poll: None,
        }
    }
    fn visible(&self) -> Vec<usize> {
        let Some(section) = self.snapshot.sections.get(self.section) else {
            return Vec::new();
        };
        let mut hidden_depth = None;
        let mut result = Vec::new();
        for (index, row) in section.rows.iter().enumerate() {
            if let Some(depth) = hidden_depth {
                if row.depth > depth {
                    continue;
                }
                hidden_depth = None;
            }
            if row.children_count > 0 && !self.expanded.contains(&row.id) && self.filter.is_empty()
            {
                hidden_depth = Some(row.depth);
            }
            if self.filter.is_empty()
                || row
                    .label
                    .to_lowercase()
                    .contains(&self.filter.to_lowercase())
            {
                result.push(index);
            }
        }
        result
    }
    fn key(&mut self, key: &str, text: Option<&str>) {
        if let Some(command) = &mut self.command {
            match key {
                "escape" => self.command = None,
                "backspace" => {
                    command.pop();
                }
                "enter" => {
                    let command = self.command.take().unwrap_or_default();
                    if let Some(query) = command.strip_prefix('/') {
                        self.filter = query.to_string();
                        self.selected = 0;
                    }
                    if let Some(name) = command.strip_prefix(':') {
                        let name = name.to_lowercase().replace('-', " ");
                        if let Some(index) = self.snapshot.sections.iter().position(|s| {
                            let title = s.title.to_lowercase().replace('-', " ");
                            title == name || (name == "needs" && title.starts_with("needs"))
                        }) {
                            self.section = index;
                            self.selected = 0;
                            self.filter.clear();
                        }
                    }
                }
                _ => {
                    if let Some(text) = text {
                        command.push_str(text);
                    }
                }
            }
            return;
        }
        match key {
            ":" | "/" => self.command = Some(key.into()),
            "j" | "down" => {
                let len = if self.explorer {
                    self.snapshot.sections.len()
                } else {
                    self.visible().len()
                };
                if len > 0 {
                    if self.explorer {
                        self.section = (self.section + 1).min(len - 1);
                        self.selected = 0;
                    } else {
                        self.selected = (self.selected + 1).min(len - 1);
                    }
                }
            }
            "k" | "up" => {
                if self.explorer {
                    self.section = self.section.saturating_sub(1);
                    self.selected = 0;
                } else {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            "enter" | "right" => {
                if self.explorer {
                    self.explorer = false;
                } else if let Some(row) = self.visible().get(self.selected).and_then(|i| {
                    self.snapshot
                        .sections
                        .get(self.section)
                        .and_then(|s| s.rows.get(*i))
                }) {
                    if row.children_count > 0 {
                        let id = row.id.clone();
                        if !self.expanded.remove(&id) {
                            self.expanded.insert(id);
                        }
                    }
                }
            }
            "escape" | "left" => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.selected = 0;
                } else {
                    self.explorer = true;
                }
            }
            _ => {}
        }
    }
}
impl Panel {
    pub fn new_openrig(bridge: crate::harness::Bridge, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::new(SESSION_ID.into(), Some("OpenRig".into()), None, bridge, cx);
        panel.items.clear();
        let fixture = crate::harness::screenshot_mode() || cfg!(test);
        let mut state = State::fixture();
        if !fixture {
            state.snapshot = Snapshot {
                sections: Vec::new(),
                error: None,
            };
            state._poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    let snapshot = cx
                        .background_executor()
                        .spawn(async { fetch_snapshot(&default_base_url()) })
                        .await;
                    if this
                        .update(cx, |panel, cx| {
                            if let Some(state) = panel.openrig.as_mut() {
                                state.snapshot = snapshot;
                                state.section = state
                                    .section
                                    .min(state.snapshot.sections.len().saturating_sub(1));
                                state.selected =
                                    state.selected.min(state.visible().len().saturating_sub(1));
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                }
            }));
        }
        panel.openrig = Some(state);
        panel
    }
    pub(super) fn render_openrig(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = self.openrig.as_ref().expect("openrig state");
        let theme = Theme::global();
        let mut explorer = div()
            .w(px(155.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .overflow_hidden();
        for (index, section) in state.snapshot.sections.iter().enumerate() {
            let active = index == state.section;
            explorer = explorer.child(
                div()
                    .id(("openrig-section", index))
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if active {
                        theme.ACCENT_DIM
                    } else {
                        theme.PANEL_BG
                    })
                    .text_color(if active { theme.TEXT } else { theme.TEXT_DIM })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if let Some(state) = this.openrig.as_mut() {
                                state.section = index;
                                state.selected = 0;
                                state.explorer = true;
                                state.filter.clear();
                            }
                            window.focus(&this.focus_handle, cx);
                            cx.notify();
                        }),
                    )
                    .child(section.title.clone()),
            );
        }
        let visible = state.visible();
        let section = state.snapshot.sections.get(state.section);
        let mut list = div()
            .id("openrig-rows")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&state.scroll)
            .flex()
            .flex_col()
            .gap_1()
            .p_2();
        if let Some(section) = section {
            for (position, index) in visible.iter().copied().enumerate() {
                let row = &section.rows[index];
                let label = format!(
                    "{}{}{}{}",
                    "  ".repeat(row.depth as usize),
                    if row.children_count > 0 {
                        if state.expanded.contains(&row.id) {
                            "▾ "
                        } else {
                            "▸ "
                        }
                    } else {
                        ""
                    },
                    row.label,
                    row.status
                        .as_ref()
                        .map(|s| format!("  {s}"))
                        .unwrap_or_default()
                );
                list = list.child(
                    div()
                        .id(("openrig-row", position))
                        .rounded_full()
                        .px_3()
                        .py_1()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .bg(if position == state.selected {
                            theme.ACCENT_DIM
                        } else {
                            theme.HEADER_BG
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                if let Some(state) = this.openrig.as_mut() {
                                    state.selected = position;
                                    state.explorer = false;
                                }
                                window.focus(&this.focus_handle, cx);
                                cx.notify();
                            }),
                        )
                        .child(label),
                );
            }
        }
        let mut detail = div()
            .w(px(220.))
            .flex_none()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(theme.TEXT_DIM);
        if let Some(row) =
            section.and_then(|s| visible.get(state.selected).and_then(|i| s.rows.get(*i)))
        {
            detail = detail.child(
                div()
                    .text_color(theme.TEXT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(row.label.clone()),
            );
            for (key, value) in &row.detail {
                detail = detail.child(div().child(format!("{key}: {value}")));
            }
        }
        let status = state
            .snapshot
            .error
            .as_ref()
            .map(|e| format!("{e}"))
            .unwrap_or_else(|| "Live · polling every 2s".into());
        let command = state.command.clone().unwrap_or_else(|| {
            if state.filter.is_empty() {
                ":section    /filter".into()
            } else {
                format!("/{}", state.filter)
            }
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if let Some(state) = this.openrig.as_mut() {
                    let key = event.keystroke.key.as_str();
                    let text = event.keystroke.key_char.as_deref();
                    let key = if state.command.is_none() {
                        text.filter(|t| *t == ":" || *t == "/").unwrap_or(key)
                    } else {
                        key
                    };
                    state.key(key, text);
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("OpenRig mission control"),
                    )
                    .child(
                        div()
                            .rounded_full()
                            .bg(theme.HEADER_BG)
                            .px_3()
                            .py_1()
                            .text_size(px(12.))
                            .text_color(theme.TEXT_DIM)
                            .child(command),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(explorer)
                    .child(list)
                    .child(detail),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_FAINT)
                    .child(format!(
                        "{status}   ·   j/k navigate   enter select   esc back"
                    )),
            )
            .into_any_element()
    }
}
