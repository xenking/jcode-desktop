use super::{
    Panel, State,
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
    pub(super) data: Option<Build>,
    pub(super) load: Option<Task<()>>,
    host: Option<String>,
    category: Kind,
    selected: Option<ItemId>,
    preview: Option<Result<Preview, String>>,
    preview_load: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl Default for BuildView {
    fn default() -> Self {
        Self {
            data: None,
            load: None,
            host: None,
            category: Kind::Rig,
            selected: None,
            preview: None,
            preview_load: None,
            scroll: ScrollHandle::default(),
        }
    }
}

impl BuildView {
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
}

fn needs_spec_preview(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Rig | Kind::Application | Kind::Agent | Kind::Workflow
    )
}

impl Panel {
    pub(super) fn build_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.openrig.as_mut() else {
            return false;
        };
        let view = &mut state.build;
        if matches!(key, "escape" | "left") {
            return view.clear_selection();
        }
        let rows: Vec<_> = view.rows(&state.filter).collect();
        let current = view
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| row.id == selected.id));
        let Some(index) = move_index(current, rows.len(), key) else {
            return false;
        };
        let id = ItemId {
            host: view.host().to_owned(),
            id: rows[index].id.clone(),
        };
        let kind = view.category;
        view.scroll.scroll_to_item(index);
        self.select_build_entry(id, kind, cx);
        true
    }

    fn select_build_entry(&mut self, id: ItemId, kind: Kind, cx: &mut Context<Self>) {
        let Some(state) = self.openrig.as_mut() else {
            return;
        };
        state.explorer = false;
        if state.build.selected.as_ref() == Some(&id) {
            return;
        }
        state.build.clear_selection();
        state.build.selected = Some(id.clone());
        if needs_spec_preview(kind) {
            if state.destination_fixture {
                state.build.preview = Some(build::fixture_preview(&id.id));
            } else {
                state.build.preview_load = Some(cx.spawn(async move |panel, cx| {
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
                    let _ = panel.update(cx, |panel, cx| {
                        let Some(view) = panel.openrig.as_mut().map(|state| &mut state.build)
                        else {
                            return;
                        };
                        if view.selected.as_ref() == Some(&id) {
                            view.preview = Some(result);
                            view.preview_load = None;
                            cx.notify();
                        }
                    });
                }));
            }
        }
        cx.notify();
    }

    pub(super) fn render_build(&self, state: &State, cx: &mut Context<Self>) -> Div {
        let view = &state.build;
        let mut rows = view.rows(&state.filter).peekable();
        let selected = view
            .selected
            .as_ref()
            .and_then(|selected| view.rows(&state.filter).find(|row| row.id == selected.id));
        let mut list = list("build-items", &view.scroll);
        if let Some(data) = &view.data {
            list = list.children(
                data.discovery_issue
                    .as_ref()
                    .map(|issue| note(issue.clone())),
            );
        }
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
            let text = if view.data.is_none() {
                "Reading the Build library…"
            } else if view.catalog().any(|(_, read)| read.is_ok()) {
                if state.filter.is_empty() {
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
            let selected = view.selected.as_ref() == Some(&id);
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
                    cx.listener(move |panel, _, window, cx| {
                        panel.select_build_entry(id.clone(), kind, cx);
                        window.focus(&panel.focus_handle, cx);
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
            .children(
                view.data
                    .iter()
                    .flat_map(|build| &build.hosts)
                    .enumerate()
                    .map(|(index, host)| {
                        let id = host.host.clone();
                        let label = if id == "local" {
                            "Local".to_owned()
                        } else {
                            id.clone()
                        };
                        let selector = format!("build-host:{id}");
                        pill(("build-host", index), label, view.host() == id)
                            .debug_selector(move || selector.clone())
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                if let Some(state) = panel.openrig.as_mut() {
                                    state.build.host = Some(id.clone());
                                    state.build.clear_selection();
                                    state.build.scroll.scroll_to_item(0);
                                    state.explorer = false;
                                }
                                window.focus(&panel.focus_handle, cx);
                                cx.notify();
                            }))
                    }),
            );
        let categories = div().flex().flex_wrap().gap_1().children(
            Kind::ALL
                .into_iter()
                .filter(|kind| {
                    *kind != Kind::Other
                        || view.data.as_ref().is_some_and(|data| {
                            data.hosts.iter().any(|host| {
                                host.catalogs
                                    .iter()
                                    .filter_map(|(_, rows)| rows.as_ref().ok())
                                    .any(|rows| {
                                        rows.items.iter().any(|row| row.kind == Kind::Other)
                                    })
                            })
                        })
                })
                .map(|kind| {
                    let label = kind.label();
                    pill(
                        ("build-category", kind as usize),
                        label,
                        view.category == kind,
                    )
                    .debug_selector(move || format!("build-category:{label}"))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.build.category = kind;
                            state.build.clear_selection();
                            state.build.scroll.scroll_to_item(0);
                            state.explorer = false;
                        }
                        window.focus(&panel.focus_handle, cx);
                        cx.notify();
                    }))
                }),
        );
        let pane = selected.map(|row| self.render_build_inspector(state, row, cx));
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
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(heading("Build library"))
                        .child(
                            pill(
                                "build-refresh",
                                if view.load.is_some() {
                                    "Reading…"
                                } else {
                                    "Refresh"
                                },
                                false,
                            )
                            .on_click(cx.listener(
                                |panel, _, _, cx| panel.load_openrig_destination(true, cx),
                            )),
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

    fn render_build_inspector(
        &self,
        state: &State,
        row: &Entry,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let view = &state.build;
        let pane = inspector("build-inspector", state.compact)
            .child(
                pill(
                    "build-back",
                    if state.compact {
                        "Back to library"
                    } else {
                        "Close inspector"
                    },
                    false,
                )
                .debug_selector(|| "build-back".into())
                .on_click(cx.listener(|panel, _, window, cx| {
                    if let Some(state) = panel.openrig.as_mut() {
                        state.build.clear_selection();
                    }
                    window.focus(&panel.focus_handle, cx);
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
        match &view.preview {
            None => pane.child(note("Reading spec preview…")),
            Some(Err(error)) => pane.child(note(format!("Preview unavailable ({error})"))),
            Some(Ok(Preview::Rig(window) | Preview::Agent(window))) => {
                let title = if row.kind == Kind::Agent {
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
}
