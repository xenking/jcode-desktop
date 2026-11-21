use super::{Panel, State, Theme};
use crate::ui_actions::OpenFleetSession;
use chrono::{DateTime, Utc};
use gpui::{
    Context, Div, FontWeight, MouseButton, ScrollHandle, SharedString, Stateful, Task, div,
    prelude::*, px,
};
use jcode_desktop_openrig::{
    SectionKind,
    fleet::Fleet,
    sections::ReadWindow,
    work::{QueueItem, Work, Workflow},
};
use std::collections::BTreeSet;

pub(super) const UNKNOWN: &str = "–";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Queue,
    Workflows,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    State,
    Owner,
}

#[derive(Clone, PartialEq, Eq)]
struct ItemId {
    tab: Tab,
    host: String,
    id: String,
}

#[derive(Clone, Copy)]
enum Row<'a> {
    Queue(&'a str, &'a QueueItem),
    Workflow(&'a str, &'a Workflow, Option<&'a str>),
}

impl<'a> Row<'a> {
    fn key(self) -> ItemId {
        let (tab, host, id) = match self {
            Self::Queue(host, row) => (Tab::Queue, host, row.qitem_id.as_str()),
            Self::Workflow(host, row, _) => (Tab::Workflows, host, row.instance_id.as_str()),
        };
        ItemId {
            tab,
            host: host.into(),
            id: id.into(),
        }
    }

    fn host(self) -> &'a str {
        match self {
            Self::Queue(host, _) | Self::Workflow(host, _, _) => host,
        }
    }

    fn title(self) -> &'a str {
        match self {
            Self::Queue(_, row) => row
                .summary
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&row.qitem_id),
            Self::Workflow(_, row, _) => &row.workflow_name,
        }
    }

    fn state(self) -> Option<&'a str> {
        match self {
            Self::Queue(_, row) => row.state.as_deref(),
            Self::Workflow(_, row, _) => row.status.as_deref(),
        }
    }

    fn owner(self) -> Option<&'a str> {
        match self {
            Self::Queue(_, row) => row.destination_session.as_deref(),
            Self::Workflow(_, _, owner) => owner,
        }
    }
}

#[derive(Default)]
pub(super) struct WorkView {
    pub(super) data: Option<Work>,
    pub(super) load: Option<Task<()>>,
    tab: Tab,
    state: Option<String>,
    owner: Option<String>,
    menu: Option<Menu>,
    selected: Option<ItemId>,
    scroll: ScrollHandle,
}

impl WorkView {
    fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.data
            .iter()
            .flat_map(|work| &work.hosts)
            .flat_map(|host| {
                let queue = host.queue.as_ref().ok();
                let workflows = host.workflows.as_ref().ok();
                queue
                    .into_iter()
                    .filter(|_| self.tab == Tab::Queue)
                    .flat_map(|rows| &rows.items)
                    .map(|row| Row::Queue(&host.host, row))
                    .chain(
                        workflows
                            .into_iter()
                            .filter(|_| self.tab == Tab::Workflows)
                            .flat_map(|rows| &rows.items)
                            .map(move |row| {
                                let mut owners = row.current_frontier.iter().flatten().map(|id| {
                                    queue?
                                        .items
                                        .iter()
                                        .find(|packet| packet.qitem_id == *id)?
                                        .destination_session
                                        .as_deref()
                                });
                                let owner = owners
                                    .next()
                                    .flatten()
                                    .filter(|first| owners.all(|owner| owner == Some(*first)));
                                Row::Workflow(&host.host, row, owner)
                            }),
                    )
            })
    }

    fn filtered(&self, query: &str) -> impl Iterator<Item = Row<'_>> {
        let query = query.to_lowercase();
        self.rows().filter(move |row| {
            self.state
                .as_deref()
                .is_none_or(|state| row.state() == Some(state))
                && self
                    .owner
                    .as_deref()
                    .is_none_or(|owner| row.owner() == Some(owner))
                && (query.is_empty()
                    || [
                        Some(row.title()),
                        Some(row.host()),
                        row.owner(),
                        row.state(),
                        match row {
                            Row::Queue(_, row) => row.body.as_deref(),
                            Row::Workflow(_, row, _) => row.current_step_id.as_deref(),
                        },
                    ]
                    .into_iter()
                    .flatten()
                    .any(|text| text.to_lowercase().contains(&query)))
        })
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.state = None;
        self.owner = None;
        self.menu = None;
        self.selected = None;
        self.scroll.scroll_to_item(0);
    }

    pub(super) fn key(&mut self, query: &str, key: &str) -> bool {
        if matches!(key, "escape" | "left") {
            return self.menu.take().is_some() || self.selected.take().is_some();
        }
        let rows: Vec<_> = self.filtered(query).map(Row::key).collect();
        let Some(index) = move_index(
            rows.iter()
                .position(|id| self.selected.as_ref() == Some(id)),
            rows.len(),
            key,
        ) else {
            return false;
        };
        self.selected = Some(rows[index].clone());
        self.scroll.scroll_to_item(index);
        true
    }
}

pub(super) fn move_index(current: Option<usize>, len: usize, key: &str) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match key {
        "down" | "j" => Some(current.map_or(0, |i| (i + 1).min(len - 1))),
        "up" | "k" => Some(current.map_or(0, |i| i.saturating_sub(1))),
        "enter" | "right" => Some(current.unwrap_or(0)),
        _ => None,
    }
}

fn age(created: Option<DateTime<Utc>>) -> String {
    let Some(created) = created else {
        return UNKNOWN.into();
    };
    let seconds = Utc::now().signed_duration_since(created).num_seconds();
    match seconds {
        ..=-1 => created.format("%Y-%m-%d %H:%M UTC").to_string(),
        0..=59 => "Just now".into(),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

pub(super) fn value(text: Option<&str>) -> &str {
    text.filter(|s| !s.trim().is_empty()).unwrap_or(UNKNOWN)
}

pub(super) fn short(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

fn open_owner(fleet: &Fleet, host: &str, owner: &str) -> Option<OpenFleetSession> {
    let source = if host == "local" {
        fleet.hosts.first()?
    } else {
        fleet.hosts.iter().find(|record| record.id == host)?
    };
    let mut matches = source
        .rigs
        .as_ref()?
        .iter()
        .flat_map(|rig| rig.pods.iter().flatten())
        .flat_map(|pod| &pod.seats)
        .filter(|seat| seat.canonical_session_name.as_deref() == Some(owner));
    let seat = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(OpenFleetSession {
        target: source.target.clone(),
        native_id: seat.jcode_session_id.clone()?,
    })
}

pub(super) fn pill(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .flex_none()
        .max_w_full()
        .truncate()
        .rounded_full()
        .bg(if selected {
            theme.ACCENT_DIM
        } else {
            theme.HEADER_BG
        })
        .text_color(if selected { theme.TEXT } else { theme.TEXT_DIM })
        .px_3()
        .py_1()
        .text_size(px(12.))
        .cursor_pointer()
        .child(label.into())
}

pub(super) fn field(label: &str, value: impl Into<SharedString>) -> Div {
    let theme = Theme::global();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(12.))
        .child(div().text_color(theme.TEXT_FAINT).child(label.to_owned()))
        .child(div().text_color(theme.TEXT).child(value.into()))
}

pub(super) fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(Theme::global().TEXT_DIM)
        .child(text.into())
}

pub(super) fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(Theme::global().TEXT)
        .child(text.into())
}

pub(super) fn list(id: &'static str, scroll: &ScrollHandle) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .gap_2()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .pr_1()
}

pub(super) fn inspector(id: &'static str, compact: bool) -> Stateful<Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .flex_col()
        .min_h_0()
        .gap_3()
        .overflow_y_scroll()
        .rounded_xl()
        .bg(theme.PANEL_BG)
        .p_3()
        .when(compact, |el| el.flex_1().min_w_0())
        .when(!compact, |el| el.w(px(310.)).flex_none())
}

pub(super) fn row_pill(
    id: impl Into<gpui::ElementId>,
    selected: bool,
    title: String,
    meta: String,
    preview: Option<String>,
) -> Stateful<Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .flex_none()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .rounded_full()
        .px_5()
        .py_2()
        .bg(if selected {
            theme.ACCENT_DIM
        } else {
            theme.HEADER_BG
        })
        .cursor_pointer()
        .child(heading(title).truncate())
        .child(note(meta).truncate())
        .children(preview.map(|text| note(text).truncate()))
}

fn source_notice<T>(host: &str, result: &Result<ReadWindow<T>, String>) -> Option<String> {
    match result {
        Ok(rows) if rows.truncated => Some(format!(
            "{host} · Showing the first {} rows",
            rows.items.len()
        )),
        Err(error) => Some(format!("{host} · Unavailable ({error})")),
        _ => None,
    }
}

impl Panel {
    pub(super) fn render_work(&self, state: &State, cx: &mut Context<Self>) -> Div {
        let view = &state.work;
        let mut rows = view.filtered(&state.filter).peekable();
        let selected = view
            .selected
            .as_ref()
            .and_then(|id| view.filtered(&state.filter).find(|row| row.key() == *id));
        let mut list = list("work-items", &view.scroll);
        if let Some(data) = &view.data {
            list = list.children(
                data.discovery_issue
                    .as_ref()
                    .map(|issue| note(issue.clone())),
            );
            for host in &data.hosts {
                let issue = match view.tab {
                    Tab::Queue => source_notice(&host.host, &host.queue),
                    Tab::Workflows => source_notice(&host.host, &host.workflows),
                };
                list = list.children(issue.map(note));
            }
        }
        if rows.peek().is_none() {
            let text = if view.data.is_none() {
                "Reading Work from OpenRig…"
            } else if view.rows().next().is_some() {
                "No matches in the loaded rows."
            } else if view.data.as_ref().is_some_and(|work| {
                work.hosts.iter().any(|host| match view.tab {
                    Tab::Queue => host.queue.is_ok(),
                    Tab::Workflows => host.workflows.is_ok(),
                })
            }) {
                match view.tab {
                    Tab::Queue => "No queue items on responding hosts.",
                    Tab::Workflows => "No workflow instances on responding hosts.",
                }
            } else {
                "No host returned this list."
            };
            list = list.child(note(text));
        }
        for (index, row) in rows.enumerate() {
            let id = row.key();
            let active = view.selected.as_ref() == Some(&id);
            let selector = format!("work-row:{}:{}", id.host, id.id);
            let owner = value(row.owner());
            let status = value(row.state());
            let (meta, preview) = match row {
                Row::Queue(host, item) => (
                    format!(
                        "{status} · {} · {} · {host}",
                        value(item.priority.as_deref()),
                        age(item.ts_created)
                    ),
                    format!(
                        "{owner} · {}",
                        short(value(item.body.as_deref()), 150)
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                ),
                Row::Workflow(host, item, _) => (
                    format!("{status} · {} · {host}", age(item.created_at)),
                    format!("Step: {} · {owner}", value(item.current_step_id.as_deref())),
                ),
            };
            list = list.child(
                row_pill(
                    ("work-row", index),
                    active,
                    row.title().to_owned(),
                    meta,
                    Some(preview),
                )
                .relative()
                .children(crate::fixture_control_targets::marker(
                    || selector.clone(),
                    || Some(self.session_id.clone()),
                ))
                .debug_selector(move || selector.clone())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |panel, _, window, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.work.selected = Some(id.clone());
                            state.work.menu = None;
                            state.explorer = false;
                        }
                        window.focus(&panel.focus_handle, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ),
            );
        }
        let toolbar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(
                [("Queue", Tab::Queue), ("Workflows", Tab::Workflows)]
                    .into_iter()
                    .map(|(label, tab)| {
                        pill(label, label, view.tab == tab)
                            .debug_selector(move || format!("work-tab:{label}"))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                if let Some(state) = panel.openrig.as_mut() {
                                    state.work.set_tab(tab);
                                    state.explorer = false;
                                }
                                window.focus(&panel.focus_handle, cx);
                                cx.notify();
                            }))
                    }),
            )
            .child(
                pill("work-projects", "Projects", false).on_click(cx.listener(
                    |panel, _, _, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.select_kind(SectionKind::Scopes);
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                pill(
                    "work-refresh",
                    if view.load.is_some() {
                        "Reading…"
                    } else {
                        "Refresh"
                    },
                    false,
                )
                .on_click(cx.listener(|panel, _, _, cx| panel.load_openrig_destination(true, cx))),
            );
        let filters = div().flex().flex_wrap().gap_1().children(
            [
                ("State", Menu::State, &view.state),
                ("Owner", Menu::Owner, &view.owner),
            ]
            .into_iter()
            .map(|(label, menu, current)| {
                pill(
                    ("work-filter", menu as usize),
                    format!("{label}: {}", current.as_deref().unwrap_or("All")),
                    current.is_some() || view.menu == Some(menu),
                )
                .debug_selector(move || format!("work-filter:{label}"))
                .on_click(cx.listener(move |panel, _, window, cx| {
                    if let Some(state) = panel.openrig.as_mut() {
                        state.work.menu = (state.work.menu != Some(menu)).then_some(menu);
                        state.explorer = false;
                    }
                    window.focus(&panel.focus_handle, cx);
                    cx.notify();
                }))
            }),
        );
        let menu = view.menu.map(|menu| {
            let choices = view
                .rows()
                .filter_map(|row| match menu {
                    Menu::State => row.state(),
                    Menu::Owner => row.owner(),
                })
                .filter(|name| !name.trim().is_empty())
                .collect::<BTreeSet<_>>();
            let choices =
                std::iter::once(None).chain(choices.into_iter().map(|name| Some(name.to_owned())));
            div()
                .id("work-filter-options")
                .flex_none()
                .max_h(px(125.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(choices.enumerate().map(|(index, choice)| {
                    let label = choice.clone().unwrap_or_else(|| "All".into());
                    let active = match menu {
                        Menu::State => view.state == choice,
                        Menu::Owner => view.owner == choice,
                    };
                    pill(("work-filter-option", index), label.clone(), active)
                        .debug_selector(move || format!("work-option:{label}"))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            if let Some(view) = panel.openrig.as_mut().map(|state| &mut state.work)
                            {
                                match menu {
                                    Menu::State => view.state = choice.clone(),
                                    Menu::Owner => view.owner = choice.clone(),
                                }
                                view.selected = None;
                                view.menu = None;
                                view.scroll.scroll_to_item(0);
                            }
                            window.focus(&panel.focus_handle, cx);
                            cx.notify();
                        }))
                }))
        });
        let pane = selected.map(|row| self.render_work_inspector(state, row, cx));
        let show_list = !state.compact || pane.is_none();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p_3()
            .gap_3()
            .flex()
            .flex_col()
            .when(show_list, |el| {
                el.child(heading("Work"))
                    .child(toolbar)
                    .child(filters)
                    .children(menu)
            })
            .child(
                div()
                    .flex()
                    .min_h_0()
                    .flex_1()
                    .gap_3()
                    .when(show_list, |el| el.child(list))
                    .children(pane),
            )
    }

    fn render_work_inspector(
        &self,
        state: &State,
        row: Row<'_>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = row
            .owner()
            .and_then(|owner| open_owner(&state.snapshot.fleet, row.host(), owner));
        let host = row.host();
        let pane = inspector("work-inspector", state.compact)
            .child(
                pill(
                    "work-back",
                    if state.compact {
                        "Back to Work"
                    } else {
                        "Close inspector"
                    },
                    false,
                )
                .debug_selector(|| "work-back".into())
                .on_click(cx.listener(|panel, _, window, cx| {
                    if let Some(state) = panel.openrig.as_mut() {
                        state.work.selected = None;
                    }
                    window.focus(&panel.focus_handle, cx);
                    cx.notify();
                })),
            )
            .child(heading(row.title().to_owned()))
            .child(note(format!("{host} · {}", value(row.state()))))
            .child(field("Owner seat", value(row.owner()).to_owned()))
            .children(open.map(|action| {
                pill("work-open-session", "Open Jcode session", true)
                    .debug_selector(|| "work-open-session".into())
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(action.clone()), cx)
                    })
            }));
        match row {
            Row::Queue(_, item) => pane
                .child(field(
                    "Priority",
                    value(item.priority.as_deref()).to_owned(),
                ))
                .child(field("Age", age(item.ts_created)))
                .child(field("Body", short(value(item.body.as_deref()), 4000)))
                .children(
                    item.blocked_on
                        .as_deref()
                        .map(|blocked| field("Blocked on", short(blocked, 300))),
                )
                .when(
                    matches!(
                        item.state.as_deref(),
                        Some("done" | "failed" | "denied" | "canceled" | "handed-off")
                    ) || item.closure_reason.is_some(),
                    |el| {
                        el.child(field(
                            "Closure reason",
                            value(item.closure_reason.as_deref()).to_owned(),
                        ))
                    },
                )
                .child(field("Item ID", item.qitem_id.clone())),
            Row::Workflow(_, item, _) => pane
                .child(field(
                    "Current step",
                    value(item.current_step_id.as_deref()).to_owned(),
                ))
                .child(field(
                    "Spec version",
                    value(item.workflow_version.as_deref()).to_owned(),
                ))
                .child(field(
                    "Created by",
                    value(item.created_by_session.as_deref()).to_owned(),
                ))
                .child(field("Age", age(item.created_at)))
                .child(field("Instance ID", item.instance_id.clone())),
        }
    }
}
