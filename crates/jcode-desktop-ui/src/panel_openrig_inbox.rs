use super::{Panel, State, Theme, detail_lines};
use chrono::Utc;
use gpui::{Context, FontWeight, MouseButton, ScrollHandle, div, prelude::*, px};
use jcode_desktop_openrig::inbox::{Inbox, Item, ItemId, Kind};

const UNKNOWN: &str = "–";
const FILTERS: [Option<Kind>; 6] = [
    None,
    Some(Kind::HumanTask),
    Some(Kind::Approval),
    Some(Kind::Permission),
    Some(Kind::Held),
    Some(Kind::SeatAttention),
];

#[derive(Default)]
pub(super) struct InboxView {
    filter: Option<Kind>,
    pub(super) selected: Option<ItemId>,
    scroll: ScrollHandle,
}

impl InboxView {
    fn rows<'a>(&self, inbox: &'a Inbox, query: &str) -> Vec<&'a Item> {
        let query = query.to_lowercase();
        inbox
            .items
            .iter()
            .filter(|item| {
                self.filter.is_none_or(|kind| kind == item.kind)
                    && (query.is_empty()
                        || [
                            Some(item.host.as_str()),
                            item.source.as_deref(),
                            item.title.as_deref(),
                            item.body.as_deref(),
                            Some(item.kind.label()),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|s| s.to_lowercase().contains(&query)))
            })
            .collect()
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
    item.title.as_deref().unwrap_or_else(|| item.kind.label())
}

fn field(theme: &Theme, name: &str, value: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(12.))
        .child(div().text_color(theme.TEXT_FAINT).child(name.to_owned()))
        .child(div().text_color(theme.TEXT).child(value.into()))
}

impl Panel {
    pub(super) fn render_inbox(&self, state: &State, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::global();
        let view = &state.inbox;
        let snapshot = state.snapshot.inbox.as_ref();
        let rows = snapshot.map_or_else(Vec::new, |inbox| view.rows(inbox, &state.filter));
        let selected = view
            .selected
            .as_ref()
            .and_then(|id| rows.iter().find(|item| &item.id == id))
            .copied();
        let mut list = div()
            .id("inbox-items")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .track_scroll(&view.scroll);

        if rows.is_empty() {
            let (heading, message) = match snapshot {
                None => (
                    "Loading Inbox",
                    "Reading OpenRig for items that need your attention.",
                ),
                Some(inbox) if !inbox.issues.is_empty() => (
                    "Inbox incomplete",
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
            let active = view.selected.as_ref() == Some(&id);
            let source = item.source.as_deref().unwrap_or(UNKNOWN);
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
                        cx.listener(move |panel, _, window, cx| {
                            if let Some(state) = panel.openrig.as_mut() {
                                state.inbox.selected = Some(id.clone());
                                state.explorer = false;
                            }
                            window.focus(&panel.focus_handle, cx);
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
                                    .child(one_line(item.body.as_deref())),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(theme.TEXT_FAINT)
                                    .child(format!("{source} · {}", item.host)),
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
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.inbox.filter = kind;
                            state.inbox.selected = None;
                        }
                        window.focus(&panel.focus_handle, cx);
                        cx.notify();
                    })),
            );
        }

        let show_list = !state.compact || selected.is_none();
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
                                        .child("Inbox"),
                                )
                                .child(
                                    div().text_size(px(12.)).text_color(theme.TEXT_DIM).child(
                                        "Approvals, human tasks, and seats waiting for you.",
                                    ),
                                ),
                        )
                        .children(
                            snapshot
                                .filter(|inbox| !inbox.issues.is_empty())
                                .map(|inbox| {
                                    div()
                                        .id("inbox-status")
                                        .rounded_xl()
                                        .p_3()
                                        .bg(theme.HEADER_BG)
                                        .text_color(theme.WARN)
                                        .text_size(px(12.))
                                        .child(inbox.issues.join("\n"))
                                }),
                        )
                        .child(filters)
                        .child(list),
                )
            })
            .children(selected.map(|item| self.render_inbox_detail(item, state.compact, theme, cx)))
    }

    fn render_inbox_detail(
        &self,
        item: &Item,
        compact: bool,
        theme: &Theme,
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
                            .child(if compact { "Back to Inbox" } else { "Close" })
                            .on_click(cx.listener(|panel, _, window, cx| {
                                if let Some(state) = panel.openrig.as_mut() {
                                    state.inbox.selected = None;
                                }
                                window.focus(&panel.focus_handle, cx);
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
        let (id, rig) = match &item.id {
            ItemId::Queue { item, .. } => (item.as_str(), None),
            ItemId::Seat { node, rig, .. } => (node.as_str(), Some(rig.as_str())),
        };
        for (key, value) in [
            ("Kind", item.kind.label()),
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
        if let Some(rig) = rig {
            detail = detail.child(field(theme, "Rig", rig.to_owned()));
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
