//! Native OpenRig mission control.
use super::Panel;
use crate::{
    openrig_feed::Feed,
    theme::{Palette, Theme},
};
use gpui::{
    App, Context, Entity, FocusHandle, FontWeight, MouseButton, ScrollHandle, Task, Window, div,
    prelude::*, px,
};
use jcode_desktop_openrig::{
    LiveEvent, Meter, SectionKind, Snapshot, Tone, default_base_url, fetch_stream_tail,
    subscribe_events,
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

fn tone_color(theme: &Palette, tone: Tone) -> gpui::Rgba {
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
fn meter_pill(theme: &Palette, meter: &Meter, width: f32) -> gpui::Div {
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

/// The OpenRig mission-control pane: what it shows, where the user is in it, and what it
/// is waiting for. The panel hosts it as a view of its own, so nothing here reaches back
/// into the panel. Everything outside the pane goes through `Panel`'s few methods below.
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
    /// The hosting panel's focus handle, so keys reach this pane.
    focus: FocusHandle,
    session_id: String,
    _poll: Option<Task<()>>,
    _events: Option<Task<()>>,
}
impl State {
    pub(super) fn screenshot_fixture(focus: FocusHandle, session_id: String) -> Self {
        let mut state = Self::fixture(focus, session_id);
        state.configure_attention_capture();
        state.configure_chat_capture();
        state
    }

    pub(super) fn fixture(focus: FocusHandle, session_id: String) -> Self {
        let snapshot = Snapshot::fixture();
        let destination_fixture = !live_verification();
        Self {
            snapshot,
            fleet: fleet::FleetView::fixture(
                std::env::var_os("JCODE_DESKTOP_SCREENSHOT_FLEET_INSPECTOR").is_some(),
            ),
            inbox: inbox::InboxView::new(inbox::Page::Inbox),
            work: work::WorkView::new(destination_fixture),
            build: build::BuildView::new(destination_fixture),
            chat: chat::ChatView::default(),
            destination_fixture,
            attention: inbox::InboxView::new(inbox::Page::Attention),
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
            focus,
            session_id,
            _poll: None,
            _events: None,
        }
    }

    /// The pane for a panel: fixture data in screenshots and tests, otherwise live reads
    /// of the daemon's event stream and of `feed`'s snapshot.
    pub(super) fn open(
        feed: Entity<Feed>,
        focus: FocusHandle,
        session_id: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut state = Self::fixture(focus, session_id);
        if crate::harness::screenshot_mode() || cfg!(test) {
            return state;
        }
        state.destination_fixture = false;
        state.work = work::WorkView::new(false);
        state.build = build::BuildView::new(false);
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
                    .update(cx, |state, cx| {
                        if !events.is_empty() {
                            state.push_events(events);
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
                let Ok((read_attention, writing, revision, shared)) =
                    this.update(cx, |state, cx| {
                        (
                            state.attention_page().is_some(),
                            state.attention_action.running(),
                            state.attention_action.revision,
                            feed.read(cx).snapshot.clone(),
                        )
                    })
                else {
                    break;
                };
                let Some(mut snapshot) = shared else {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    continue;
                };
                let (snapshot, tail) = cx
                    .background_executor()
                    .spawn(async move {
                        let base = default_base_url();
                        if read_attention && !writing {
                            snapshot.load_attention();
                        }
                        (snapshot, fetch_stream_tail(&base, 5))
                    })
                    .await;
                if this
                    .update(cx, |state, cx| {
                        state.absorb(snapshot, tail.ok(), writing, revision);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
        state
    }

    /// Keep the newest 50 live events.
    fn push_events(&mut self, events: Vec<LiveEvent>) {
        for event in events {
            self.events.push_back(event);
        }
        while self.events.len() > 50 {
            self.events.pop_front();
        }
    }

    /// Take a polled snapshot, keeping the queue the user is acting on if an action
    /// started or finished while it was being read.
    fn absorb(
        &mut self,
        mut snapshot: Snapshot,
        tail: Option<Vec<LiveEvent>>,
        writing: bool,
        revision: u64,
    ) {
        if self.attention_action.running() || writing || self.attention_action.revision != revision
        {
            snapshot.inbox = self.snapshot.inbox.take().or(snapshot.inbox);
        }
        self.attention_action
            .keep_confirmation_for(snapshot.inbox.as_ref());
        self.snapshot = snapshot;
        if let Some(tail) = tail {
            self.events = tail.into();
        }
        self.section = self
            .section
            .min(self.snapshot.sections.len().saturating_sub(1));
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
    }

    pub(super) fn width_changed(&self, width: gpui::Pixels) -> bool {
        self.compact != (width < px(740.))
    }

    pub(super) fn set_width(&mut self, width: gpui::Pixels, cx: &mut Context<Self>) {
        self.compact = width < px(740.);
        cx.notify();
    }

    pub(super) fn inspect_fleet_seat(
        &mut self,
        action: &crate::ui_actions::InspectFleetSeat,
        cx: &mut Context<Self>,
    ) {
        self.section = self
            .snapshot
            .sections
            .iter()
            .position(|s| s.kind == SectionKind::Fleet)
            .unwrap_or(0);
        self.filter.clear();
        self.command = None;
        self.fleet.inspect(
            &self.snapshot.fleet,
            &action.host_id,
            &action.rig_id,
            &action.seat_id,
        );
        cx.notify();
    }

    fn load_destination(&mut self, refresh: bool, cx: &mut Context<Self>) {
        if self.destination_fixture {
            return;
        }
        let started = match self.kind() {
            Some(SectionKind::Work) => self.work.reload(refresh, cx, |this| &mut this.work),
            Some(SectionKind::Build) => self.build.reload(refresh, cx, |this| &mut this.build),
            _ => false,
        };
        if started {
            cx.notify();
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

impl State {
    fn select_row(&mut self, position: usize) {
        self.selected = position;
        self.explorer = false;
    }

    #[cfg(test)]
    fn inspected_fleet_key(&self) -> Option<&str> {
        self.fleet.selected_key()
    }

    /// Keys for whichever pane is showing. True when the pane used the key.
    fn pane_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.is_fleet()
            && !self.explorer
            && self.command.is_none()
            && matches!(key, "j" | "k" | "up" | "down" | "enter" | "right")
        {
            if let Some(action) = self.fleet.key(&self.snapshot.fleet, &self.filter, key) {
                window.dispatch_action(Box::new(action), cx);
            }
            return true;
        }
        if self.attention_page().is_some() && self.attention_action.handle_key(key) {
            window.focus(&self.focus, cx);
            return true;
        }
        if let Some(page) = self.attention_page()
            && !self.explorer
            && self.command.is_none()
            && self.inbox_key(page, key)
        {
            return true;
        }
        if !self.explorer && self.command.is_none() {
            let kind = self.kind();
            if kind == Some(SectionKind::Work) && self.work.key(&self.filter, key) {
                return true;
            }
            if kind == Some(SectionKind::Build) && self.build_key(key, cx) {
                return true;
            }
            if kind == Some(SectionKind::Chat) && self.chat_key(key, window, cx) {
                return true;
            }
        }
        false
    }

    fn on_key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind() == Some(SectionKind::Chat) && self.chat.composer_focused(window, cx) {
            return;
        }
        if self.attention_action.input_focused(window, cx) {
            return;
        }
        let key = event.keystroke.key.as_str();
        let text = event.keystroke.key_char.as_deref();
        let key = if self.command.is_none() {
            text.filter(|t| *t == ":" || *t == "/").unwrap_or(key)
        } else {
            key
        };
        if self.pane_key(key, window, cx) {
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.is_fleet()
            && self.command.is_none()
            && self.fleet.properties
            && matches!(key, "escape" | "left")
        {
            self.fleet.close_properties();
            cx.notify();
            cx.stop_propagation();
            return;
        }
        self.key(key, text);
        if !self.explorer && matches!(key, "j" | "k" | "up" | "down" | "enter" | "right") {
            self.scroll.scroll_to_item(self.selected);
        }
        self.load_destination(false, cx);
        cx.notify();
        cx.stop_propagation();
    }
}

/// What the rest of the panel may ask of the pane.
impl Panel {
    #[cfg(test)]
    pub(crate) fn inspected_fleet_key<'a>(&'a self, cx: &'a App) -> Option<&'a str> {
        self.openrig.as_ref()?.read(cx).inspected_fleet_key()
    }

    pub(crate) fn inspect_fleet_seat(
        &mut self,
        action: &crate::ui_actions::InspectFleetSeat,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = &self.openrig {
            pane.update(cx, |pane, cx| pane.inspect_fleet_seat(action, cx));
        }
    }

    pub(crate) fn openrig_width_changed(&self, width: gpui::Pixels, cx: &App) -> bool {
        self.openrig
            .as_ref()
            .is_some_and(|pane| pane.read(cx).width_changed(width))
    }

    pub(crate) fn set_openrig_width(&mut self, width: gpui::Pixels, cx: &mut Context<Self>) {
        if let Some(pane) = &self.openrig {
            pane.update(cx, |pane, cx| pane.set_width(width, cx));
        }
    }

    pub(crate) fn show_chat_open_error(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = &self.openrig {
            pane.update(cx, |pane, cx| pane.show_chat_open_error(cx));
        }
    }

    pub fn new_openrig(bridge: crate::harness::Bridge, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::new(SESSION_ID.into(), Some("OpenRig".into()), None, bridge, cx);
        panel.rig_demand = Some(panel.rig_feed.read(cx).demand(None));
        panel.items.clear();
        let (feed, focus) = (panel.rig_feed.clone(), panel.focus_handle.clone());
        let session = panel.session_id.clone();
        panel.openrig = Some(cx.new(|cx| State::open(feed, focus, session, cx)));
        panel
    }

    pub(super) fn render_openrig(&self) -> gpui::AnyElement {
        self.openrig
            .clone()
            .expect("openrig pane")
            .into_any_element()
    }
}

impl Render for State {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        let is_fleet = self.is_fleet();
        let is_work = self.kind() == Some(SectionKind::Work);
        let is_build = self.kind() == Some(SectionKind::Build);
        let is_chat = self.kind() == Some(SectionKind::Chat);
        let attention_page = self.attention_page();
        let compact = self.compact;
        let mut explorer = div().flex_none().flex().gap_1().p_2().overflow_hidden();
        explorer = if compact {
            explorer.w_full().flex_wrap()
        } else {
            explorer.w(px(143.)).flex_col()
        };
        for (index, section) in self.snapshot.sections.iter().enumerate() {
            let active = index == self.section;
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
                            this.attention_action.cancel();
                            this.select(index);
                            this.load_destination(false, cx);
                            window.focus(&this.focus, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(section.title.clone()),
            );
        }
        let (top_sections, side_sections) = if compact {
            (self.show_sections.then_some(explorer), None)
        } else {
            (None, Some(explorer))
        };
        let visible = self.visible();
        let section = self.snapshot.sections.get(self.section);
        let mut list = div()
            .id("openrig-rows")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .gap_1()
            .p_2();
        if let Some(section) = section {
            for (position, index) in visible.iter().copied().enumerate() {
                let row = &section.rows[index];
                let marker = if row.children_count > 0 {
                    if self.is_open(row) { "▾ " } else { "▸ " }
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
                    .bg(if position == self.selected {
                        theme.ACCENT_DIM
                    } else {
                        theme.HEADER_BG
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.select_row(position);
                            window.focus(&this.focus, cx);
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
            section.and_then(|s| visible.get(self.selected).and_then(|i| s.rows.get(*i)))
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
            .then_some(self.snapshot.error.as_ref())
            .flatten()
            .cloned()
            .unwrap_or_else(|| {
                if (is_work || is_build) && !self.destination_fixture {
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
        let command = self.command.clone().unwrap_or_else(|| {
            if self.filter.is_empty() {
                ":section    /filter".into()
            } else {
                format!("/{}", self.filter)
            }
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                this.on_key(event, window, cx)
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
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_sections = !this.show_sections;
                                    window.focus(&this.focus, cx);
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
                    .when(is_fleet, |el| el.child(self.render_fleet(cx)))
                    .when(is_work, |el| el.child(self.render_work(cx)))
                    .when(is_build, |el| el.child(self.render_build(cx)))
                    .when(is_chat, |el| el.child(self.render_chat(cx)))
                    .when_some(attention_page, |el, page| {
                        el.child(self.render_inbox(page, cx))
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
                self.events
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

    #[gpui::test]
    fn default_open_groups_show_children_until_toggled(cx: &mut gpui::TestAppContext) {
        let focus = cx.update(|cx| cx.focus_handle());
        let mut state = State::fixture(focus, SESSION_ID.into());
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
