//! Native OpenRig mission control.
use super::Panel;
use crate::theme::Theme;
use gpui::{Context, FontWeight, MouseButton, ScrollHandle, Task, Window, div, prelude::*, px};
use jcode_desktop_openrig::{
    LiveEvent, Meter, SectionKind, Snapshot, Tone, default_base_url, fetch_snapshot,
    fetch_stream_tail, subscribe_events,
};
use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};

#[path = "panel_openrig_attention.rs"]
mod attention;
#[path = "panel_openrig_build.rs"]
mod build;
#[path = "panel_openrig_chat.rs"]
mod chat;
#[path = "panel_openrig_fleet.rs"]
mod fleet;
#[path = "panel_openrig_inbox.rs"]
mod inbox;
#[path = "panel_openrig_work.rs"]
mod work;

pub(crate) const SESSION_ID: &str = "openrig://mission-control";

fn live_verification() -> bool {
    crate::fixture_control_targets::enabled()
        && std::env::var("JCODE_DESKTOP_CONTROL_OPENRIG_LIVE").as_deref() == Ok("1")
}

fn destination_base_url() -> String {
    if live_verification() {
        "http://127.0.0.1:7433".into()
    } else {
        default_base_url()
    }
}

fn tone_color(theme: &Theme, tone: Tone) -> gpui::Rgba {
    match tone {
        Tone::Ok => theme.OK,
        Tone::Warn => theme.WARN,
        Tone::Error => theme.ERROR,
        Tone::Dim => theme.TEXT_FAINT,
        Tone::Neutral => theme.TEXT_DIM,
    }
}

/// A progress pill: a fully rounded track filled to the served percentage, with the served
/// words beside it. An unknown percentage draws an empty track, never a guessed fill.
fn meter_pill(theme: &Theme, meter: &Meter, width: f32) -> gpui::Div {
    let color = tone_color(theme, meter.tone);
    let fill = meter.fraction().unwrap_or(0.);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(width))
                .h(px(8.))
                .rounded_full()
                .bg(theme.PANEL_BG)
                .child(
                    div()
                        .w(gpui::relative(fill))
                        .h_full()
                        .rounded_full()
                        .bg(color),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(color)
                .child(meter.label.clone()),
        )
}

fn detail_lines(key: &str, value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(fields) if !fields.is_empty() => {
            for (name, value) in fields {
                detail_lines(&format!("{key} · {name}"), value, out);
            }
        }
        serde_json::Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                detail_lines(&format!("{key} {}", index + 1), value, out);
            }
        }
        serde_json::Value::String(text) => out.push((key.into(), text.clone())),
        value => out.push((key.into(), value.to_string())),
    }
}

pub(super) struct State {
    snapshot: Snapshot,
    fleet: fleet::FleetView,
    inbox: inbox::InboxView,
    work: work::WorkView,
    build: build::BuildView,
    chat: chat::ChatView,
    destination_fixture: bool,
    attention: inbox::InboxView,
    attention_action: attention::ActionState,
    compact: bool,
    show_sections: bool,
    section: usize,
    selected: usize,
    filter: String,
    command: Option<String>,
    explorer: bool,
    /// Rows the user toggled away from their default (open or closed).
    toggled: HashSet<String>,
    scroll: ScrollHandle,
    events: VecDeque<LiveEvent>,
    event_lifetime: std::sync::Arc<()>,
    _poll: Option<Task<()>>,
    _events: Option<Task<()>>,
}
impl State {
    pub(super) fn screenshot_fixture() -> Self {
        let mut state = Self::fixture();
        state.configure_attention_capture();
        state.configure_chat_capture();
        state
    }

    pub(super) fn fixture() -> Self {
        let snapshot = Snapshot::fixture();
        let mut work = work::WorkView::default();
        let destination_fixture = !live_verification();
        work.data = destination_fixture.then(jcode_desktop_openrig::work::Work::fixture);
        let mut build = build::BuildView::default();
        build.data = destination_fixture.then(jcode_desktop_openrig::build::Build::fixture);
        Self {
            snapshot,
            fleet: fleet::FleetView::fixture(
                std::env::var_os("JCODE_DESKTOP_SCREENSHOT_FLEET_INSPECTOR").is_some(),
            ),
            inbox: inbox::InboxView::default(),
            work,
            build,
            chat: chat::ChatView::default(),
            destination_fixture,
            attention: inbox::InboxView::default(),
            attention_action: attention::ActionState::default(),
            compact: false,
            show_sections: false,
            section: 0,
            selected: 0,
            filter: String::new(),
            command: None,
            explorer: true,
            toggled: HashSet::new(),
            scroll: ScrollHandle::new(),
            events: [LiveEvent {
                ts: "12:04".into(),
                kind: "seat.started".into(),
                summary: "Seat started".into(),
                rig: Some("example".into()),
            }]
            .into(),
            event_lifetime: std::sync::Arc::new(()),
            _poll: None,
            _events: None,
        }
    }
    fn is_fleet(&self) -> bool {
        self.snapshot
            .sections
            .get(self.section)
            .is_none_or(|section| section.kind == SectionKind::Fleet)
    }
    fn attention_page(&self) -> Option<inbox::Page> {
        self.snapshot
            .sections
            .get(self.section)
            .and_then(|section| match section.kind {
                SectionKind::Inbox => Some(inbox::Page::Inbox),
                SectionKind::Needs => Some(inbox::Page::Attention),
                _ => None,
            })
    }
    fn kind(&self) -> Option<SectionKind> {
        self.snapshot
            .sections
            .get(self.section)
            .map(|section| section.kind)
    }

    fn select(&mut self, index: usize) {
        self.section = index;
        self.selected = 0;
        self.filter.clear();
        self.show_sections = false;
        self.explorer = true;
    }

    fn select_kind(&mut self, kind: SectionKind) {
        if let Some(index) = self
            .snapshot
            .sections
            .iter()
            .position(|section| section.kind == kind)
        {
            self.select(index);
        }
    }
    fn is_open(&self, row: &jcode_desktop_openrig::Row) -> bool {
        row.default_open != self.toggled.contains(&row.id)
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
            if row.children_count > 0 && !self.is_open(row) && self.filter.is_empty() {
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
                            title == name
                                || (name == "needs" && title.starts_with("needs"))
                                || (name == "specs" && s.kind == SectionKind::Build)
                        }) {
                            self.select(index);
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
                }) && row.children_count > 0
                {
                    let id = row.id.clone();
                    if !self.toggled.remove(&id) {
                        self.toggled.insert(id);
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
    fn load_openrig_destination(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let Some(state) = self.openrig.as_mut() else {
            return;
        };
        if state.destination_fixture {
            return;
        }
        match state.kind() {
            Some(SectionKind::Work)
                if state.work.load.is_none() && (refresh || state.work.data.is_none()) =>
            {
                state.work.load = Some(cx.spawn(async |panel, cx| {
                    let work = cx
                        .background_executor()
                        .spawn(async {
                            jcode_desktop_openrig::work::fetch_work(&destination_base_url())
                        })
                        .await;
                    let _ = panel.update(cx, |panel, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.work.data = Some(work);
                            state.work.load = None;
                            cx.notify();
                        }
                    });
                }));
                cx.notify();
            }
            Some(SectionKind::Build)
                if state.build.load.is_none() && (refresh || state.build.data.is_none()) =>
            {
                state.build.load = Some(cx.spawn(async |panel, cx| {
                    let build = cx
                        .background_executor()
                        .spawn(async {
                            jcode_desktop_openrig::build::fetch_build(&destination_base_url())
                        })
                        .await;
                    let _ = panel.update(cx, |panel, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.build.data = Some(build);
                            state.build.load = None;
                            cx.notify();
                        }
                    });
                }));
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn openrig_width_changed(&self, width: gpui::Pixels) -> bool {
        self.openrig
            .as_ref()
            .is_some_and(|state| state.compact != (width < px(740.)))
    }

    pub(crate) fn set_openrig_width(&mut self, width: gpui::Pixels, cx: &mut Context<Self>) {
        if let Some(state) = self.openrig.as_mut() {
            state.compact = width < px(740.);
            cx.notify();
        }
    }

    pub fn new_openrig(bridge: crate::harness::Bridge, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::new(SESSION_ID.into(), Some("OpenRig".into()), None, bridge, cx);
        panel.items.clear();
        let fixture = crate::harness::screenshot_mode() || cfg!(test);
        let mut state = State::fixture();
        if !fixture {
            state.destination_fixture = false;
            state.work.data = None;
            state.build.data = None;
            state.events.clear();
            let (tx, rx) = std::sync::mpsc::sync_channel(128);
            let lifetime = std::sync::Arc::downgrade(&state.event_lifetime);
            std::thread::spawn(move || {
                let mut delay = 1;
                // Reads time out every 15 s, so a closed pane ends this thread promptly.
                while lifetime.upgrade().is_some() {
                    let mut received = false;
                    let result = subscribe_events(
                        &default_base_url(),
                        // Checked on every frame, keepalives included: an idle
                        // stream must not outlive a closed pane.
                        || lifetime.upgrade().is_none(),
                        |event| {
                            received = true;
                            tx.send(event).is_ok()
                        },
                    );
                    if result.is_ok() || lifetime.upgrade().is_none() {
                        break;
                    }
                    if received {
                        delay = 1;
                    }
                    std::thread::sleep(Duration::from_secs(delay));
                    delay = (delay * 2).min(30);
                }
            });
            state._events = Some(cx.spawn(async move |this, cx| {
                loop {
                    let events: Vec<_> = rx.try_iter().collect();
                    if this
                        .update(cx, |panel, cx| {
                            if events.is_empty() {
                                return;
                            }
                            if let Some(state) = panel.openrig.as_mut() {
                                for event in events {
                                    state.events.push_back(event);
                                }
                                while state.events.len() > 50 {
                                    state.events.pop_front();
                                }
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                }
            }));
            state.snapshot = Snapshot::default();
            state._poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    let Ok((read_attention, writing, revision)) = this.update(cx, |panel, _| {
                        panel.openrig.as_ref().map_or((false, false, 0), |state| {
                            (
                                state.attention_page().is_some(),
                                state.attention_action.running(),
                                state.attention_action.revision,
                            )
                        })
                    }) else {
                        break;
                    };
                    let (snapshot, tail) = cx
                        .background_executor()
                        .spawn(async move {
                            let base = default_base_url();
                            let mut snapshot = fetch_snapshot(&base);
                            if read_attention && !writing {
                                snapshot.load_attention();
                            }
                            (snapshot, fetch_stream_tail(&base, 5))
                        })
                        .await;
                    if this
                        .update(cx, |panel, cx| {
                            if let Some(state) = panel.openrig.as_mut() {
                                let mut snapshot = snapshot;
                                if state.attention_action.running()
                                    || writing
                                    || state.attention_action.revision != revision
                                {
                                    snapshot.inbox = state.snapshot.inbox.take().or(snapshot.inbox);
                                }
                                state
                                    .attention_action
                                    .keep_confirmation_for(snapshot.inbox.as_ref());
                                state.snapshot = snapshot;
                                if let Ok(tail) = tail {
                                    state.events = tail.into();
                                }
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
        let is_fleet = state.is_fleet();
        let is_work = state.kind() == Some(SectionKind::Work);
        let is_build = state.kind() == Some(SectionKind::Build);
        let is_chat = state.kind() == Some(SectionKind::Chat);
        let attention_page = state.attention_page();
        let compact = state.compact;
        let mut explorer = div().flex_none().flex().gap_1().p_2().overflow_hidden();
        explorer = if compact {
            explorer.w_full().flex_wrap()
        } else {
            explorer.w(px(143.)).flex_col()
        };
        for (index, section) in state.snapshot.sections.iter().enumerate() {
            let active = index == state.section;
            explorer = explorer.child(
                div()
                    .id(("openrig-section", index))
                    .debug_selector(|| format!("openrig-section:{}", section.title))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || format!("openrig-section:{}", section.title),
                        || Some(self.session_id.clone()),
                    ))
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
                                state.attention_action.cancel();
                                state.select(index);
                            }
                            this.load_openrig_destination(false, cx);
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(section.title.clone()),
            );
        }
        let (top_sections, side_sections) = if compact {
            (state.show_sections.then_some(explorer), None)
        } else {
            (None, Some(explorer))
        };
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
                let marker = if row.children_count > 0 {
                    if state.is_open(row) { "▾ " } else { "▸ " }
                } else {
                    ""
                };
                let mut line = div()
                    .id(("openrig-row", position))
                    .rounded_full()
                    .pl(px(12. + 14. * f32::from(row.depth)))
                    .pr_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_3()
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
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("{marker}{}", row.label)),
                    );
                if let Some(status) = &row.status {
                    line = line.child(
                        div()
                            .flex_none()
                            .max_w(px(220.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(tone_color(theme, row.tone))
                            .child(status.clone()),
                    );
                }
                if let Some(meter) = &row.meter {
                    line = line.child(meter_pill(theme, meter, 64.));
                }
                list = list.child(line);
            }
        }
        let mut detail = div()
            .id("openrig-detail")
            .when(compact, |el| el.w(gpui::relative(0.42)))
            .when(!compact, |el| el.w(px(320.)))
            .min_w_0()
            .flex_none()
            .overflow_y_scroll()
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
            if let Some(meter) = &row.meter {
                detail = detail.child(meter_pill(theme, meter, 160.));
            }
            for (key, value) in &row.detail {
                let mut lines = Vec::new();
                let structured = value.starts_with(['{', '[']);
                if let Some(json) = structured
                    .then(|| serde_json::from_str::<serde_json::Value>(value).ok())
                    .flatten()
                {
                    detail_lines(key, &json, &mut lines);
                } else {
                    lines.push((key.clone(), value.clone()));
                }
                for (label, text) in lines {
                    let item = div()
                        .rounded_xl()
                        .bg(theme.HEADER_BG)
                        .px_3()
                        .py_1()
                        .flex()
                        .flex_col()
                        .child(div().text_color(theme.TEXT_FAINT).child(label))
                        .child(div().text_color(theme.TEXT).child(text));
                    detail = detail.child(item);
                }
            }
        }
        let status = (attention_page.is_none() && !is_work && !is_build && !is_chat)
            .then_some(state.snapshot.error.as_ref())
            .flatten()
            .cloned()
            .unwrap_or_else(|| {
                if (is_work || is_build) && !state.destination_fixture {
                    "Read-only · Refresh to check for changes".into()
                } else if crate::harness::screenshot_mode() {
                    if is_chat && live_verification()
                        || std::env::var("JCODE_DESKTOP_SCREENSHOT_OPENRIG_ATTENTION").as_deref()
                            == Ok("live")
                    {
                        "Live OpenRig · read-only capture".into()
                    } else {
                        "Fixture data".into()
                    }
                } else {
                    "Live · polling every 2s".into()
                }
            });
        let command = state.command.clone().unwrap_or_else(|| {
            if state.filter.is_empty() {
                ":section    /filter".into()
            } else {
                format!("/{}", state.filter)
            }
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if let Some(state) = this.openrig.as_mut() {
                    if state.kind() == Some(SectionKind::Chat)
                        && state.chat.composer_focused(window, cx)
                    {
                        return;
                    }
                    let key = event.keystroke.key.as_str();
                    let text = event.keystroke.key_char.as_deref();
                    let key = if state.command.is_none() {
                        text.filter(|t| *t == ":" || *t == "/").unwrap_or(key)
                    } else {
                        key
                    };
                    if state.is_fleet()
                        && !state.explorer
                        && state.command.is_none()
                        && matches!(key, "j" | "k" | "up" | "down" | "enter" | "right")
                    {
                        if let Some(action) =
                            state.fleet.key(&state.snapshot.fleet, &state.filter, key)
                        {
                            window.dispatch_action(Box::new(action), cx);
                        }
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    if state.attention_page().is_some() && state.attention_action.handle_key(key) {
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    if let Some(page) = state.attention_page()
                        && !state.explorer
                        && state.command.is_none()
                    {
                        let view = match page {
                            inbox::Page::Inbox => &mut state.inbox,
                            inbox::Page::Attention => &mut state.attention,
                        };
                        if view.key(page, state.snapshot.inbox.as_ref(), &state.filter, key) {
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                    }
                    if !state.explorer && state.command.is_none() {
                        let kind = state.kind();
                        if kind == Some(SectionKind::Work) && state.work.key(&state.filter, key) {
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                        if kind == Some(SectionKind::Build) && this.build_key(key, cx) {
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                        if kind == Some(SectionKind::Chat) && this.chat_key(key, window, cx) {
                            cx.notify();
                            cx.stop_propagation();
                            return;
                        }
                    }
                }
                if let Some(state) = this.openrig.as_mut() {
                    let key = event.keystroke.key.as_str();
                    let text = event.keystroke.key_char.as_deref();
                    let key = if state.command.is_none() {
                        text.filter(|t| *t == ":" || *t == "/").unwrap_or(key)
                    } else {
                        key
                    };
                    if state.is_fleet()
                        && state.command.is_none()
                        && state.fleet.properties
                        && matches!(key, "escape" | "left")
                    {
                        state.fleet.properties = false;
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    state.key(key, text);
                    if !state.explorer
                        && matches!(key, "j" | "k" | "up" | "down" | "enter" | "right")
                    {
                        state.scroll.scroll_to_item(state.selected);
                    }
                    this.load_openrig_destination(false, cx);
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
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("OpenRig"))
                    .when(compact, |header| {
                        header.child(
                            div()
                                .id("openrig-section-menu")
                                .debug_selector(|| "openrig-section-menu".into())
                                .relative()
                                .children(crate::fixture_control_targets::marker(
                                    || "openrig-section-menu".into(),
                                    || Some(self.session_id.clone()),
                                ))
                                .rounded_full()
                                .bg(theme.HEADER_BG)
                                .px_3()
                                .py_1()
                                .text_size(px(11.))
                                .cursor_pointer()
                                .child("Sections")
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    if let Some(state) = panel.openrig.as_mut() {
                                        state.show_sections = !state.show_sections;
                                    }
                                    window.focus(&panel.focus_handle, cx);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .rounded_full()
                            .min_w_0()
                            .truncate()
                            .bg(theme.HEADER_BG)
                            .px_3()
                            .py_1()
                            .text_size(px(12.))
                            .text_color(theme.TEXT_DIM)
                            .child(command),
                    ),
            )
            .children(top_sections)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(side_sections)
                    .when(is_fleet, |el| el.child(self.render_fleet(state, cx)))
                    .when(is_work, |el| el.child(self.render_work(state, cx)))
                    .when(is_build, |el| el.child(self.render_build(state, cx)))
                    .when(is_chat, |el| el.child(self.render_chat(state, cx)))
                    .when_some(attention_page, |el, page| {
                        el.child(self.render_inbox(state, page, cx))
                    })
                    .when(
                        !is_fleet && attention_page.is_none() && !is_work && !is_build && !is_chat,
                        |el| el.child(list).child(detail),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_FAINT)
                    .truncate()
                    .child(if compact {
                        status
                    } else {
                        format!("{status}   ·   j/k navigate   Enter activate   Esc back")
                    }),
            )
            .children(
                state
                    .events
                    .iter()
                    .rev()
                    .take(if compact { 0 } else { 3 })
                    .map(|event| {
                        div()
                            .px_4()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .child(format!(
                                "{}  {}  {}{}",
                                event.ts,
                                event.kind,
                                event.summary,
                                event
                                    .rig
                                    .as_ref()
                                    .map(|rig| format!(" · {rig}"))
                                    .unwrap_or_default()
                            ))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod openrig_tests {
    use super::*;

    fn row(id: &str, depth: u8, children: usize, open: bool) -> jcode_desktop_openrig::Row {
        jcode_desktop_openrig::Row {
            id: id.into(),
            label: id.into(),
            depth,
            children_count: children,
            default_open: open,
            ..jcode_desktop_openrig::Row::default()
        }
    }

    #[test]
    fn default_open_groups_show_children_until_toggled() {
        let mut state = State::fixture();
        state.snapshot.sections = vec![jcode_desktop_openrig::Section {
            kind: jcode_desktop_openrig::SectionKind::Usage,
            title: "Usage".into(),
            rows: vec![
                row("provider", 0, 1, true),
                row("window", 1, 0, false),
                row("closed", 0, 1, false),
                row("hidden", 1, 0, false),
            ],
        }];
        state.section = 0;
        assert_eq!(state.visible(), vec![0, 1, 2]);
        state.toggled.insert("provider".into());
        state.toggled.insert("closed".into());
        assert_eq!(state.visible(), vec![0, 2, 3]);
    }

    #[test]
    fn meter_fill_is_clamped_and_unknown_is_empty() {
        let meter = |percent| Meter {
            label: String::new(),
            percent,
            tone: Tone::Ok,
        };
        assert_eq!(meter(Some(150.)).fraction(), Some(1.));
        assert_eq!(meter(Some(-3.)).fraction(), Some(0.));
        assert_eq!(meter(Some(f64::NAN)).fraction(), None);
        assert_eq!(meter(None).fraction(), None);
    }

    #[test]
    fn nested_usage_details_keep_window_information() {
        let mut lines = Vec::new();
        detail_lines(
            "windows",
            &serde_json::json!([{ "window": "weekly", "remainingPercent": 42, "resetsAt": "tomorrow" }]),
            &mut lines,
        );
        assert!(lines.contains(&("windows 1 · remainingPercent".into(), "42".into())));
        assert!(lines.contains(&("windows 1 · resetsAt".into(), "tomorrow".into())));
    }
}
