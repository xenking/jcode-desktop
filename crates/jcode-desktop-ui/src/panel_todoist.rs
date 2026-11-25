//! The Todoist panel: its tasks and the actions on them.
use super::*;

impl Panel {
    pub fn new_todoist(bridge: Bridge, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::new(
            "todoist://tasks".into(),
            Some("todos".into()),
            None,
            bridge,
            cx,
        );
        panel.items.clear();
        panel.todoist = Some(TodoistPanelState {
            load: TodoistLoadState::Loading,
            tasks: Vec::new(),
            projects: Vec::new(),
            selected_project: None,
            busy_tasks: HashSet::new(),
            scroll: ScrollHandle::new(),
            scroll_layout: None,
        });
        let weak = cx.weak_entity();
        panel.input = cx.new(|cx| {
            PromptInput::new(
                cx,
                "add a task, for example: Review PR tomorrow",
                move |content, _, _, app| {
                    let _ = weak.update(app, |panel, cx| panel.create_todoist_task(content, cx));
                },
            )
        });
        panel.refresh_todoist(cx);
        panel
    }

    pub(super) fn refresh_todoist(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.todoist.as_mut() else {
            return;
        };
        state.load = TodoistLoadState::Loading;
        cx.notify();
        self.run_todoist(
            cx,
            |client| Ok((client.tasks()?, client.projects()?)),
            |panel, result, cx| {
                let Some(state) = panel.todoist.as_mut() else {
                    return;
                };
                match result {
                    Ok((tasks, projects)) => {
                        state.tasks = tasks;
                        state.projects = projects;
                        state.load = TodoistLoadState::Ready;
                    }
                    Err(error) => state.load = TodoistLoadState::Error(error),
                }
                cx.notify();
            },
        );
    }

    pub(super) fn create_todoist_task(&mut self, content: String, cx: &mut Context<Self>) {
        let content = content.trim().to_owned();
        if content.is_empty() {
            return;
        }
        let project_id = self
            .todoist
            .as_ref()
            .and_then(|state| state.selected_project.clone());
        self.run_todoist(
            cx,
            move |client| {
                client.create_task(&CreateTask {
                    content: &content,
                    project_id: project_id.as_deref(),
                    ..CreateTask::default()
                })
            },
            |panel, result, cx| {
                let Some(state) = panel.todoist.as_mut() else {
                    return;
                };
                match result {
                    Ok(task) => {
                        state.tasks.push(task);
                        state.load = TodoistLoadState::Ready;
                    }
                    Err(error) => state.load = TodoistLoadState::Error(error),
                }
                cx.notify();
            },
        );
    }

    pub(super) fn complete_todoist_task(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(state) = self.todoist.as_mut() {
            state.busy_tasks.insert(id.clone());
        }
        self.run_todoist(
            cx,
            move |client| client.close_task(&id).map(|_| id),
            |panel, result, cx| {
                let Some(state) = panel.todoist.as_mut() else {
                    return;
                };
                match result {
                    Ok(id) => state.tasks.retain(|task| task.id != id),
                    Err(error) => state.load = TodoistLoadState::Error(error),
                }
                state.busy_tasks.clear();
                cx.notify();
            },
        );
    }

    pub(super) fn run_todoist<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        operation: impl FnOnce(TodoistClient) -> crate::todoist::Result<T> + Send + 'static,
        apply: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    ) {
        let (tx, rx) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("jcode-todoist".into())
            .spawn(move || {
                let result = TodoistClient::from_env()
                    .and_then(operation)
                    .map_err(|error| error.to_string());
                let _ = tx.send_blocking(result);
            })
            .expect("spawn Todoist worker");
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.recv().await {
                let _ = this.update(cx, |panel, cx| apply(panel, result, cx));
            }
        })
        .detach();
    }

    pub(super) fn render_todoist(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let state = self.todoist.as_mut().expect("Todoist state");
        let layout = (
            state.tasks.len(),
            state.projects.len(),
            state.selected_project.clone(),
            window.viewport_size(),
        );
        // Repaint after layout, including asynchronous loads, filtering and resize.
        // Never poll for geometry on every idle frame.
        if state.scroll_layout.as_ref() != Some(&layout) {
            state.scroll_layout = Some(layout);
            let panel = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = panel.update(cx, |_, cx| cx.notify());
            });
        }
        let selected = state.selected_project.clone();
        let mut tasks = state
            .tasks
            .iter()
            .filter(|task| {
                selected
                    .as_ref()
                    .is_none_or(|project| &task.project_id == project)
            })
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| std::cmp::Reverse(task.priority));

        let mut projects = div().flex().flex_wrap().gap_1().child(
            div()
                .id("todoist-project-all")
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .bg(if selected.is_none() {
                    Theme::global().ACCENT_DIM
                } else {
                    Theme::global().HEADER_BG
                })
                .text_size(px(11.))
                .child("All")
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|panel, _, _, cx| {
                        if let Some(state) = panel.todoist.as_mut() {
                            state.selected_project = None;
                        }
                        cx.notify();
                    }),
                ),
        );
        for (index, project) in state.projects.iter().cloned().enumerate() {
            let project_id = project.id.clone();
            let is_selected = selected.as_deref() == Some(project.id.as_str());
            projects = projects.child(
                div()
                    .id(("todoist-project", index))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(if is_selected {
                        Theme::global().ACCENT_DIM
                    } else {
                        Theme::global().HEADER_BG
                    })
                    .hover(|el| el.bg(Theme::global().ACCENT_DIM))
                    .text_size(px(11.))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(todoist_named_color(project.color.as_deref())),
                    )
                    .child(project.name)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |panel, _, _, cx| {
                            if let Some(state) = panel.todoist.as_mut() {
                                state.selected_project = Some(project_id.clone());
                            }
                            cx.notify();
                        }),
                    ),
            );
        }

        let mut list = div()
            .id("todoist-task-list")
            .debug_selector(|| "todoist-task-list".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .track_scroll(&state.scroll)
            .px_4()
            .pb_4()
            .flex()
            .flex_col()
            .gap_1();
        if tasks.is_empty() {
            let message = match &state.load {
                TodoistLoadState::Loading => "Loading Todoist…".to_owned(),
                TodoistLoadState::Ready => "No open tasks in this view.".to_owned(),
                TodoistLoadState::Error(error) => format!(
                    "{error}\n\nSet TODOIST_API_TOKEN, then refresh. The token is never written to Jcode config."
                ),
            };
            list = list.child(
                div()
                    .p_4()
                    .whitespace_normal()
                    .text_color(match state.load {
                        TodoistLoadState::Error(_) => Theme::global().ERROR,
                        _ => Theme::global().TEXT_DIM,
                    })
                    .child(message),
            );
        }
        for (index, task) in tasks.into_iter().enumerate() {
            let id = task.id.clone();
            let busy = state.busy_tasks.contains(&task.id);
            let project_name = state
                .projects
                .iter()
                .find(|project| project.id == task.project_id)
                .map(|p| p.name.clone());
            let priority = todoist_priority_color(task.priority);
            list = list.child(
                div()
                    .id(("todoist-task", index))
                    .debug_selector(move || format!("todoist-task-{index}"))
                    .flex_none()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(Theme::global().PANEL_BORDER_IDLE)
                    .bg(Theme::global().BG)
                    .hover(|el| {
                        el.bg(Theme::global().HEADER_BG)
                            .border_color(Theme::global().PANEL_BORDER)
                    })
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .id(("todoist-complete", index))
                            .mt(px(2.))
                            .size(px(16.))
                            .rounded_full()
                            .border_2()
                            .border_color(priority)
                            .cursor_pointer()
                            .when(busy, |el| el.bg(priority))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.complete_todoist_task(id.clone(), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(13.)).child(task.content))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_size(px(10.))
                                    .text_color(Theme::global().TEXT_DIM)
                                    .when_some(task.due.map(|due| due.string), |el, due| {
                                        el.child(format!("◷ {due}"))
                                    })
                                    .when_some(project_name, |el, name| {
                                        el.child(format!("# {name}"))
                                    })
                                    .when(task.priority > 1, |el| {
                                        el.child(format!("P{}", 5 - task.priority))
                                    }),
                            ),
                    ),
            );
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.input.read(cx).editor_focus())
            .child(
                div()
                    .debug_selector(|| "todoist-header".into())
                    .flex_none()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Todos"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(Theme::global().TEXT_DIM)
                                    .child(format!(
                                        "{} open · synced with Todoist",
                                        state.tasks.len()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("todoist-refresh")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(Theme::global().TEXT_DIM)
                            .hover(|el| el.bg(Theme::global().HEADER_BG))
                            .child("↻ refresh")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|panel, _, _, cx| panel.refresh_todoist(cx)),
                            ),
                    ),
            )
            .child(div().flex_none().px_4().pb_3().child(projects))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(list)
                    .child(crate::scrollbar::vertical(
                        &state.scroll,
                        "todoist-scrollbar",
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "todoist-composer".into())
                    .flex_none()
                    .border_t_1()
                    .border_color(Theme::global().PANEL_BORDER)
                    .p_3()
                    .child(self.input.clone()),
            )
            .into_any_element()
    }
}
