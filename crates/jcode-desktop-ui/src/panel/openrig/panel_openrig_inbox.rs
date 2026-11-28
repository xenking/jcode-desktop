use super::{Palette, State, Theme, default_base_url, detail_lines};
use chrono::Utc;
use gpui::{Context, FontWeight, MouseButton, ScrollHandle, div, prelude::*, px};
use jcode_desktop_openrig::{
    attention::Kind,
    inbox::{Inbox, Item, ItemId},
};

const UNKNOWN: &str = "–";
const FILTERS: [Option<Kind>; 8] = [
    None,
    Some(Kind::HumanTask),
    Some(Kind::Approval),
    Some(Kind::Permission),
    Some(Kind::Held),
    Some(Kind::SeatAttention),
    Some(Kind::Parked),
    Some(Kind::Startup),
];

#[derive(Clone, Copy)]
pub(super) enum Page {
    Inbox,
    Attention,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Attention => "Needs attention",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::Inbox => "Requests, approvals, and seat activity across hosts.",
            Self::Attention => {
                "Seat issues, permissions, held messages, and parked work across hosts."
            }
        }
    }
}

/// One page's filter, selection and scroll position.
pub(super) struct InboxView {
    page: Page,
    filter: Option<Kind>,
    selected: Option<ItemId>,
    show_sources: bool,
    scroll: ScrollHandle,
}

impl InboxView {
    pub(super) fn new(page: Page) -> Self {
        Self {
            page,
            filter: None,
            selected: None,
            show_sources: false,
            scroll: ScrollHandle::default(),
        }
    }

    /// Select `id`. False when it already was.
    fn select(&mut self, id: ItemId) -> bool {
        let moved = self.selected.as_ref() != Some(&id);
        self.selected = Some(id);
        moved
    }

    fn close(&mut self) {
        self.selected = None;
    }

    /// The row among `rows` that is selected, if it is still listed.
    fn chosen<'a>(&self, rows: &[&'a Item]) -> Option<&'a Item> {
        let id = self.selected.as_ref()?;
        rows.iter().find(|item| &item.id == id).copied()
    }

    fn is_selected(&self, id: &ItemId) -> bool {
        self.selected.as_ref() == Some(id)
    }

    /// The scrolling column the rows are listed in.
    fn list(&self) -> gpui::Stateful<gpui::Div> {
        div()
            .id("inbox-items")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
    }

    fn filter_by(&mut self, kind: Option<Kind>) {
        self.filter = kind;
        self.selected = None;
    }

    fn toggle_sources(&mut self) {
        self.show_sources = !self.show_sources;
    }

    /// Open with `selected` chosen and the source issues expanded, as a capture shows it.
    fn staged(&mut self, selected: Option<ItemId>, show_sources: bool) {
        if selected.is_some() {
            self.selected = selected;
        }
        self.show_sources |= show_sources;
    }

    fn rows<'a>(&self, inbox: &'a Inbox, query: &str) -> Vec<&'a Item> {
        let query = query.to_lowercase();
        let mut rows: Vec<_> = inbox
            .items
            .iter()
            .filter(|item| {
                self.filter.is_none_or(|kind| kind == item.kind())
                    && (query.is_empty()
                        || [
                            Some(item.host.as_str()),
                            item.source.as_deref(),
                            item.title.as_deref(),
                            item.body.as_deref(),
                            item.rig.as_deref(),
                            item.seat.as_deref(),
                            item.reason.as_deref(),
                            Some(item.kind().label()),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|s| s.to_lowercase().contains(&query)))
            })
            .collect();
        if matches!(self.page, Page::Attention) {
            rows.sort_by_key(|row| row.actions().is_empty());
        }
        rows
    }

    pub(super) fn key(&mut self, inbox: Option<&Inbox>, query: &str, key: &str) -> bool {
        if matches!(key, "escape" | "left") {
            return self.selected.take().is_some();
        }
        let Some(inbox) = inbox else { return false };
        let rows = self.rows(inbox, query);
        if rows.is_empty() || !matches!(key, "j" | "down" | "k" | "up" | "enter" | "right") {
            return false;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|item| &item.id == id));
        let index = match (key, current) {
            ("j" | "down", Some(i)) => (i + 1).min(rows.len() - 1),
            ("k" | "up", Some(i)) => i.saturating_sub(1),
            (_, i) => i.unwrap_or(0),
        };
        self.selected = Some(rows[index].id.clone());
        self.scroll.scroll_to_item(index);
        true
    }
}

fn age(item: &Item) -> String {
    let Some(time) = item.created_at.or(item.updated_at) else {
        return UNKNOWN.into();
    };
    let minutes = Utc::now().signed_duration_since(time).num_minutes();
    match minutes {
        ..=-1 => time.format("%Y-%m-%d %H:%M UTC").to_string(),
        0 => "Just now".into(),
        1..=59 => format!("{minutes}m"),
        60..=1439 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

fn one_line(text: Option<&str>) -> String {
    let Some(text) = text else {
        return UNKNOWN.into();
    };
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return UNKNOWN.into();
    };
    let mut chars = first
        .chars()
        .chain(words.flat_map(|word| " ".chars().chain(word.chars())));
    let mut preview: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

fn title(item: &Item) -> &str {
    item.title.as_deref().unwrap_or_else(|| item.kind().label())
}

fn field(theme: &Palette, name: &str, value: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(12.))
        .child(div().text_color(theme.TEXT_FAINT).child(name.to_owned()))
        .child(div().text_color(theme.TEXT).child(value.into()))
}

impl State {
    fn inbox_view(&self, page: Page) -> &InboxView {
        match page {
            Page::Inbox => &self.inbox,
            Page::Attention => &self.attention,
        }
    }

    fn inbox_view_mut(&mut self, page: Page) -> &mut InboxView {
        match page {
            Page::Inbox => &mut self.inbox,
            Page::Attention => &mut self.attention,
        }
    }

    /// Route a key to the page's list. True when the page used it.
    pub(super) fn inbox_key(&mut self, page: Page, key: &str) -> bool {
        let inbox = self.snapshot.inbox.as_ref();
        let view = match page {
            Page::Inbox => &mut self.inbox,
            Page::Attention => &mut self.attention,
        };
        view.key(inbox, &self.filter, key)
    }

    /// Set up the Needs attention page for a screenshot capture.
    pub(super) fn configure_attention_capture(&mut self) {
        let mode = std::env::var("JCODE_DESKTOP_SCREENSHOT_OPENRIG_ATTENTION");
        match mode.as_deref() {
            Ok("live") => {
                self.snapshot = jcode_desktop_openrig::fetch_snapshot(&default_base_url());
                self.snapshot.load_attention();
                self.attention.staged(None, true);
                self.events.clear();
            }
            Ok("inspector" | "confirm") => {
                if let Some(row) = self
                    .snapshot
                    .inbox
                    .iter()
                    .flat_map(|inbox| &inbox.items)
                    .find(|row| !row.actions().is_empty())
                {
                    self.attention.staged(Some(row.id.clone()), false);
                    if mode.as_deref() == Ok("confirm")
                        && let Some(target) = self.snapshot.attention_target(&row.id).cloned()
                    {
                        self.attention_action.stage(
                            row.id.clone(),
                            row.actions()[0].clone(),
                            target,
                            None,
                        );
                    }
                }
            }
            _ => return,
        }
        self.select_kind(jcode_desktop_openrig::SectionKind::Needs);
    }

    pub(super) fn render_inbox(&self, page: Page, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::global();
        let view = self.inbox_view(page);
        let snapshot = self.snapshot.inbox.as_ref();
        let rows = snapshot.map_or_else(Vec::new, |inbox| view.rows(inbox, &self.filter));
        let selected = view.chosen(&rows);
        let mut list = view.list();

        if rows.is_empty() {
            let (heading, message) = match snapshot {
                None => (
                    "Reading OpenRig",
                    "Reading OpenRig for items that need your attention.",
                ),
                Some(inbox) if !inbox.waits_loaded() => (
                    "Reading OpenRig",
                    "Checking seats, held messages, and parked work.",
                ),
                Some(inbox) if !inbox.issues.is_empty() => (
                    "Some sources are unavailable",
                    "No matching items from the sources that responded.",
                ),
                Some(inbox) if inbox.items.is_empty() => {
                    ("All caught up", "Nothing needs your attention right now.")
                }
                Some(_) => ("No matching items", "Try another filter."),
            };
            list = list.child(
                div()
                    .id("inbox-empty")
                    .p_5()
                    .rounded_xl()
                    .bg(theme.HEADER_BG)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(heading))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.TEXT_DIM)
                            .child(message),
                    ),
            );
        }

        for (index, item) in rows.into_iter().enumerate() {
            let id = item.id.clone();
            let active = view.is_selected(&id);
            let seat = item.seat.as_deref().unwrap_or(UNKNOWN);
            let rig = item.rig.as_deref().unwrap_or(UNKNOWN);
            list = list.child(
                div()
                    .id(("inbox-item", index))
                    .debug_selector(|| format!("inbox-item:{:?}", item.id))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_full()
                    .bg(if active {
                        theme.ACCENT_DIM
                    } else {
                        theme.HEADER_BG
                    })
                    .hover(|style| style.bg(theme.PANEL_BG))
                    .px_4()
                    .py_2()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.attention_action.cancel();
                            this.inbox_view_mut(page).select(id.clone());
                            this.explorer = false;
                            window.focus(&this.focus, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title(item).to_owned()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme.TEXT_DIM)
                                    .child(one_line(
                                        item.reason.as_deref().or(item.body.as_deref()),
                                    )),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(theme.TEXT_FAINT)
                                    .child(format!("{} · {rig} · {seat}", item.host)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .child(age(item)),
                    ),
            );
        }

        let mut filters = div().flex().flex_wrap().gap_1();
        for (index, kind) in FILTERS.into_iter().enumerate() {
            filters = filters.child(
                div()
                    .id(("inbox-filter", index))
                    .debug_selector(|| format!("inbox-filter:{}", kind.map_or("All", Kind::label)))
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .text_size(px(11.))
                    .cursor_pointer()
                    .bg(if view.filter == kind {
                        theme.ACCENT_DIM
                    } else {
                        theme.HEADER_BG
                    })
                    .child(kind.map_or("All", Kind::label))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.attention_action.cancel();
                        this.inbox_view_mut(page).filter_by(kind);
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })),
            );
        }

        let show_list = !self.compact || selected.is_none();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .gap_3()
            .p_3()
            .when(show_list, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(page.title()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.TEXT_DIM)
                                        .child(page.subtitle()),
                                ),
                        )
                        .children(self.attention_action.notice(snapshot).map(|(text, error)| {
                            div()
                                .text_size(px(12.))
                                .text_color(if error { theme.ERROR } else { theme.TEXT_DIM })
                                .child(text.to_owned())
                        }))
                        .children(snapshot.map(|inbox| {
                            let hosts = inbox
                                .hosts
                                .iter()
                                .filter(|host| host.status == "ok")
                                .map(|host| host.host_id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .text_size(px(12.))
                                .when(!hosts.is_empty(), |el| {
                                    el.child(
                                        div()
                                            .text_color(theme.TEXT_DIM)
                                            .child(format!("Queue read: {hosts}")),
                                    )
                                })
                                .when(!inbox.issues.is_empty(), |el| {
                                    el.child(
                                        div().flex().child(
                                            div()
                                                .id("inbox-show-sources")
                                                .px_3()
                                                .py_1()
                                                .rounded_full()
                                                .bg(theme.HEADER_BG)
                                                .text_color(theme.WARN)
                                                .cursor_pointer()
                                                .child(if view.show_sources {
                                                    "Hide source issues".into()
                                                } else {
                                                    format!(
                                                        "Source issues ({})",
                                                        inbox.issues.len()
                                                    )
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.inbox_view_mut(page).toggle_sources();
                                                    cx.notify();
                                                })),
                                        ),
                                    )
                                })
                                .when(view.show_sources && !inbox.issues.is_empty(), |el| {
                                    el.child(
                                        div()
                                            .id("inbox-status")
                                            .max_h(px(144.))
                                            .overflow_y_scroll()
                                            .p_2()
                                            .text_color(theme.WARN)
                                            .child(inbox.issues.join("\n")),
                                    )
                                })
                        }))
                        .child(filters)
                        .child(list),
                )
            })
            .children(
                selected.map(|item| self.render_inbox_detail(item, page, self.compact, theme, cx)),
            )
    }

    fn render_inbox_detail(
        &self,
        item: &Item,
        page: Page,
        compact: bool,
        theme: &Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut detail = div()
            .id("inbox-inspector")
            .debug_selector(|| "inbox-inspector".into())
            .when(compact, |el| el.flex_1())
            .when(!compact, |el| el.w(px(300.)).flex_none())
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_xl()
            .bg(theme.HEADER_BG)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Item details"),
                    )
                    .child(
                        div()
                            .id("inbox-close")
                            .rounded_full()
                            .px_3()
                            .py_1()
                            .bg(theme.PANEL_BG)
                            .text_size(px(11.))
                            .cursor_pointer()
                            .child(if compact { "Back to list" } else { "Close" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.attention_action.cancel();
                                this.inbox_view_mut(page).close();
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title(item).to_owned()),
            )
            .child(self.render_attention_actions(item, theme, cx))
            .child(field(
                theme,
                "Body",
                item.body.as_deref().unwrap_or(UNKNOWN).to_owned(),
            ));

        let time = |value: Option<chrono::DateTime<Utc>>| {
            value
                .map(|value| value.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| UNKNOWN.into())
        };
        let id = match &item.id {
            ItemId::Queue { item, .. } => item,
            ItemId::Seat { node, .. } | ItemId::Parked { node, .. } => node,
            ItemId::HeldMessage { message, .. } => message,
        };
        for (key, value) in [
            ("Kind", item.kind().label()),
            (
                "Reason class",
                item.reason_class.map_or(UNKNOWN, |class| class.label()),
            ),
            ("Reason", item.reason.as_deref().unwrap_or(UNKNOWN)),
            ("Rig", item.rig.as_deref().unwrap_or(UNKNOWN)),
            ("Seat", item.seat.as_deref().unwrap_or(UNKNOWN)),
            ("Source seat", item.source.as_deref().unwrap_or(UNKNOWN)),
            ("Host", item.host.as_str()),
            (
                "Destination",
                item.destination.as_deref().unwrap_or(UNKNOWN),
            ),
            ("State", item.state.as_deref().unwrap_or(UNKNOWN)),
            ("Blocked on", item.blocked_on.as_deref().unwrap_or(UNKNOWN)),
            ("Evidence", item.evidence.as_deref().unwrap_or(UNKNOWN)),
            ("ID", id),
        ] {
            detail = detail.child(field(theme, key, value.to_owned()));
        }
        detail = detail
            .child(field(theme, "Created", time(item.created_at)))
            .child(field(theme, "Updated", time(item.updated_at)));
        if let Some(questions) = &item.questions {
            let mut lines = Vec::new();
            for (i, question) in questions.iter().enumerate() {
                detail_lines(&format!("Question {}", i + 1), question, &mut lines);
            }
            detail = detail.children(
                lines
                    .into_iter()
                    .map(|(name, value)| field(theme, &name, value)),
            );
        }
        detail
    }
}
