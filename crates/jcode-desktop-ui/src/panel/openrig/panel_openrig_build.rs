use super::{
    State,
    work::{field, heading, inspector, list, move_index, note, pill, row_pill, short, value},
};
use gpui::{Context, Div, MouseButton, ScrollHandle, Stateful, Task, div, prelude::*, px};
use jcode_desktop_openrig::build::{self, Build, Entry, Kind, Preview};

#[derive(Clone, PartialEq, Eq)]
struct ItemId {
    host: String,
    id: String,
}

pub(super) struct BuildView {
    data: Option<Build>,
    load: Option<Task<()>>,
    host: Option<String>,
    category: Kind,
    selected: Option<ItemId>,
    preview: Option<Result<Preview, String>>,
    preview_load: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl BuildView {
    pub(super) fn new(fixture: bool) -> Self {
        Self {
            data: fixture.then(Build::fixture),
            load: None,
            host: None,
            category: Kind::Rig,
            selected: None,
            preview: None,
            preview_load: None,
            scroll: ScrollHandle::default(),
        }
    }

    fn host(&self) -> &str {
        self.host.as_deref().unwrap_or("local")
    }

    fn catalog(
        &self,
    ) -> impl Iterator<
        Item = &(
            build::Catalog,
            Result<jcode_desktop_openrig::sections::ReadWindow<Entry>, String>,
        ),
    > {
        self.data
            .iter()
            .flat_map(|data| &data.hosts)
            .filter(|host| host.host == self.host())
            .flat_map(|host| &host.catalogs)
            .filter(|(catalog, _)| catalog.includes(self.category))
    }

    fn rows(&self, query: &str) -> impl Iterator<Item = &Entry> {
        let query = query.to_lowercase();
        self.catalog()
            .filter_map(|(_, result)| result.as_ref().ok())
            .flat_map(|window| &window.items)
            .filter(move |row| {
                row.kind == self.category
                    && (query.is_empty()
                        || [
                            Some(row.name.as_str()),
                            row.summary.as_deref(),
                            row.source.as_deref(),
                            row.version.as_deref(),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|text| text.to_lowercase().contains(&query)))
            })
    }

    fn clear_selection(&mut self) -> bool {
        self.preview = None;
        self.preview_load = None;
        self.selected.take().is_some()
    }

    /// The entry a navigation key moves to, scrolled into view.
    fn step(&mut self, filter: &str, key: &str) -> Option<(ItemId, Kind)> {
        let rows: Vec<_> = self.rows(filter).collect();
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| row.id == selected.id));
        let index = move_index(current, rows.len(), key)?;
        let id = ItemId {
            host: self.host().to_owned(),
            id: rows[index].id.clone(),
        };
        self.scroll.scroll_to_item(index);
        Some((id, self.category))
    }

    fn is_selected(&self, id: &ItemId) -> bool {
        self.selected.as_ref() == Some(id)
    }

    fn selected_entry(&self, filter: &str) -> Option<&Entry> {
        let selected = self.selected.as_ref()?;
        self.rows(filter).find(|row| row.id == selected.id)
    }

    /// The scrolling column the entries are listed in.
    fn list(&self) -> Stateful<Div> {
        list("build-items", &self.scroll)
    }

    fn has_data(&self) -> bool {
        self.data.is_some()
    }

    fn discovery_issue(&self) -> Option<&str> {
        self.data.as_ref()?.discovery_issue.as_deref()
    }

    fn host_ids(&self) -> impl Iterator<Item = &str> {
        self.data
            .iter()
            .flat_map(|build| &build.hosts)
            .map(|host| host.host.as_str())
    }

    /// Whether any host lists an entry that fits no category.
    fn has_uncategorised(&self) -> bool {
        self.data.as_ref().is_some_and(|data| {
            data.hosts.iter().any(|host| {
                host.catalogs
                    .iter()
                    .filter_map(|(_, rows)| rows.as_ref().ok())
                    .any(|rows| rows.items.iter().any(|row| row.kind == Kind::Other))
            })
        })
    }

    fn is_loading(&self) -> bool {
        self.load.is_some()
    }

    /// Select `id` and, for entries with a spec, show its preview: at once for fixture
    /// data, otherwise once the fetch lands. False when `id` was already selected.
    fn select_entry<V: 'static>(
        &mut self,
        id: ItemId,
        kind: Kind,
        fixture: bool,
        cx: &mut Context<V>,
        view: fn(&mut V) -> &mut Self,
    ) -> bool {
        if !self.select(&id) {
            return false;
        }
        if needs_spec_preview(kind) {
            if fixture {
                self.preview = Some(build::fixture_preview(&id.id));
            } else {
                self.preview_load = Some(cx.spawn(async move |this, cx| {
                    let request_id = id.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            build::fetch_preview(
                                &super::destination_base_url(),
                                &request_id.host,
                                &request_id.id,
                            )
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if view(this).finish_preview(&id, result) {
                            cx.notify();
                        }
                    });
                }));
            }
        }
        true
    }

    /// Read the library when it has not been read or when `refresh` asks again, unless a
    /// read is already running. True when one started.
    pub(super) fn reload<V: 'static>(
        &mut self,
        refresh: bool,
        cx: &mut Context<V>,
        view: fn(&mut V) -> &mut Self,
    ) -> bool {
        if self.load.is_some() || !(refresh || self.data.is_none()) {
            return false;
        }
        self.load = Some(cx.spawn(async move |this, cx| {
            let build = cx
                .background_executor()
                .spawn(async { build::fetch_build(&super::destination_base_url()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let view = view(this);
                view.data = Some(build);
                view.load = None;
                cx.notify();
            });
        }));
        true
    }

    /// The spec preview of the selected entry, under the entry's own details in `pane`.
    fn preview_section(&self, pane: Stateful<Div>, kind: Kind) -> Stateful<Div> {
        match self.preview.as_ref() {
            None => pane.child(note("Reading spec preview…")),
            Some(Err(error)) => pane.child(note(format!("Preview unavailable ({error})"))),
            Some(Ok(Preview::Rig(window) | Preview::Agent(window))) => {
                let title = if kind == Kind::Agent {
                    "Profiles and defaults"
                } else {
                    "Members"
                };
                pane.child(heading(title))
                    .children(window.items.iter().map(|member| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_size(px(12.))
                            .child(heading(member.name.clone()))
                            .child(note(format!(
                                "Runtime: {}",
                                value(member.runtime.as_deref())
                            )))
                            .child(note(format!(
                                "Declared model: {}",
                                value(member.model.as_deref())
                            )))
                            .children(
                                member
                                    .agent_ref
                                    .as_ref()
                                    .map(|agent| note(format!("Agent: {agent}"))),
                            )
                            .children(
                                member
                                    .profile
                                    .as_ref()
                                    .map(|profile| note(format!("Profile: {profile}"))),
                            )
                    }))
                    .when(window.truncated, |el| {
                        el.child(note(format!(
                            "Showing the first {} entries",
                            window.items.len()
                        )))
                    })
            }
            Some(Ok(Preview::Workflow(window))) => pane
                .child(heading("Steps"))
                .children(window.items.iter().map(|step| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_size(px(12.))
                        .child(heading(step.step_id.clone()))
                        .child(note(format!("Role: {}", value(step.role.as_deref()))))
                        .children(step.objective.as_deref().map(|text| note(short(text, 500))))
                }))
                .when(window.truncated, |el| {
                    el.child(note(format!(
                        "Showing the first {} steps",
                        window.items.len()
                    )))
                }),
        }
    }

    fn select_host(&mut self, host: String) {
        self.host = Some(host);
        self.clear_selection();
        self.scroll.scroll_to_item(0);
    }

    fn select_category(&mut self, category: Kind) {
        self.category = category;
        self.clear_selection();
        self.scroll.scroll_to_item(0);
    }

    /// Select `id` and drop the previous preview. False when it was already selected.
    fn select(&mut self, id: &ItemId) -> bool {
        if self.selected.as_ref() == Some(id) {
            return false;
        }
        self.clear_selection();
        self.selected = Some(id.clone());
        true
    }

    /// Keep a fetched preview only when its entry is still the one selected.
    fn finish_preview(&mut self, id: &ItemId, result: Result<Preview, String>) -> bool {
        if self.selected.as_ref() != Some(id) {
            return false;
        }
        self.preview = Some(result);
        self.preview_load = None;
        true
    }
}

fn needs_spec_preview(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Rig | Kind::Application | Kind::Agent | Kind::Workflow
    )
}

impl State {
    pub(super) fn build_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if matches!(key, "escape" | "left") {
            return self.build.clear_selection();
        }
        let Some((id, kind)) = self.build.step(&self.filter, key) else {
            return false;
        };
        self.select_build_entry(id, kind, cx);
        true
    }

    fn select_build_entry(&mut self, id: ItemId, kind: Kind, cx: &mut Context<Self>) {
        self.explorer = false;
        if self
            .build
            .select_entry(id, kind, self.destination_fixture, cx, |this| {
                &mut this.build
            })
        {
            cx.notify();
        }
    }

    pub(super) fn render_build(&self, cx: &mut Context<Self>) -> Div {
        let view = &self.build;
        let mut rows = view.rows(&self.filter).peekable();
        let selected = view.selected_entry(&self.filter);
        let mut list = view.list();
        list = list.children(view.discovery_issue().map(|issue| note(issue.to_owned())));
        for (catalog, result) in view.catalog() {
            list = list.children(match result {
                Ok(rows) if rows.truncated => Some(note(format!(
                    "{} · Showing the first {} catalog rows",
                    catalog.label(),
                    rows.items.len()
                ))),
                Err(error) => Some(note(format!(
                    "{} · Unavailable on {} ({error})",
                    catalog.label(),
                    view.host()
                ))),
                _ => None,
            });
        }
        if rows.peek().is_none() {
            let text = if !view.has_data() {
                "Reading the Build library…"
            } else if view.catalog().any(|(_, read)| read.is_ok()) {
                if self.filter.is_empty() {
                    "No entries in this category on the selected host."
                } else {
                    "No matches in this category."
                }
            } else {
                "No catalog data for this category."
            };
            list = list.child(note(text));
        }
        for (index, row) in rows.enumerate() {
            let id = ItemId {
                host: view.host().to_owned(),
                id: row.id.clone(),
            };
            let selector = format!("build-row:{}:{}", id.host, id.id);
            let kind = row.kind;
            let selected = view.is_selected(&id);
            let meta = format!(
                "Version: {} · {}",
                value(row.version.as_deref()),
                value(row.source.as_deref())
            );
            list = list.child(
                row_pill(
                    ("build-row", index),
                    selected,
                    row.name.clone(),
                    meta,
                    row.summary.as_deref().map(|text| short(text, 150)),
                )
                .relative()
                .children(crate::fixture_control_targets::marker(
                    || selector.clone(),
                    || Some(self.session_id.clone()),
                ))
                .debug_selector(move || selector.clone())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.select_build_entry(id.clone(), kind, cx);
                        window.focus(&this.focus, cx);
                        cx.stop_propagation();
                    }),
                ),
            );
        }
        let hosts = div()
            .id("build-hosts")
            .flex_none()
            .flex()
            .gap_1()
            .overflow_x_scroll()
            .min_w_0()
            .children(view.host_ids().enumerate().map(|(index, id)| {
                let id = id.to_owned();
                let label = if id == "local" {
                    "Local".to_owned()
                } else {
                    id.clone()
                };
                let selector = format!("build-host:{id}");
                pill(("build-host", index), label, view.host() == id)
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.build.select_host(id.clone());
                        this.explorer = false;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }))
            }));
        let categories = div().flex().flex_wrap().gap_1().children(
            Kind::ALL
                .into_iter()
                .filter(|kind| *kind != Kind::Other || view.has_uncategorised())
                .map(|kind| {
                    let label = kind.label();
                    pill(
                        ("build-category", kind as usize),
                        label,
                        view.category == kind,
                    )
                    .debug_selector(move || format!("build-category:{label}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.build.select_category(kind);
                        this.explorer = false;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }))
                }),
        );
        let pane = selected.map(|row| self.render_build_inspector(row, cx));
        let show_list = !self.compact || pane.is_none();
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p_3()
            .gap_3()
            .flex()
            .flex_col()
            .when(show_list, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(heading("Build library"))
                        .child(
                            pill(
                                "build-refresh",
                                if view.is_loading() {
                                    "Reading…"
                                } else {
                                    "Refresh"
                                },
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.load_destination(true, cx)),
                            ),
                        ),
                )
                .child(hosts)
                .child(categories)
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

    fn render_build_inspector(&self, row: &Entry, cx: &mut Context<Self>) -> Stateful<Div> {
        let view = &self.build;
        let pane = inspector("build-inspector", self.compact)
            .child(
                pill(
                    "build-back",
                    if self.compact {
                        "Back to library"
                    } else {
                        "Close inspector"
                    },
                    false,
                )
                .debug_selector(|| "build-back".into())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.build.clear_selection();
                    window.focus(&this.focus, cx);
                    cx.notify();
                })),
            )
            .child(heading(row.name.clone()))
            .child(note(format!("{} · {}", row.kind.label(), view.host())))
            .child(field("Version", value(row.version.as_deref()).to_owned()))
            .child(field("Source", value(row.source.as_deref()).to_owned()))
            .children(
                row.summary
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| note(short(text, 1200))),
            );
        if !needs_spec_preview(row.kind) {
            return pane
                .when(matches!(row.kind, Kind::Plugin | Kind::AgentImage), |el| {
                    el.child(field(
                        "Runtimes",
                        value(Some(&row.runtimes.join(", "))).to_owned(),
                    ))
                })
                .when(row.kind == Kind::Plugin, |el| {
                    el.child(field(
                        "Skill count",
                        row.skill_count
                            .map_or_else(|| super::work::UNKNOWN.into(), |count| count.to_string()),
                    ))
                })
                .when_some(row.files.as_ref(), |el, files| {
                    el.child(heading("Files"))
                        .children(files.items.iter().map(|path| note(path.clone())))
                        .when(files.truncated, |el| {
                            el.child(note(format!(
                                "Showing the first {} files",
                                files.items.len()
                            )))
                        })
                });
        }
        view.preview_section(pane, row.kind)
    }
}
