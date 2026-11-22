use super::*;
use gpui::{AnyElement, Div, SharedString, Task, UniformListScrollHandle};
use jcode_desktop_model::context_files::FileIndex;
use std::sync::Arc;
use std::time::SystemTime;

const DEFAULT_WIDTH: f32 = 280.0;
pub(super) const TAB_WIDTH: f32 = 32.0;

gpui::actions!(workspace, [ToggleFiles]);

#[derive(Clone)]
struct VisibleFile {
    path: PathBuf,
    label: SharedString,
    folder: Option<bool>,
    depth: usize,
}

pub(super) struct FilesPane {
    pub(super) open: bool,
    width: f32,
    pub(super) resizing: bool,
    root: Option<PathBuf>,
    rows: Arc<[VisibleFile]>,
    rows_revision: Option<u64>,
    scroll: UniformListScrollHandle,
    search: Option<Entity<PromptInput>>,
    query: String,
    index: Option<Arc<FileIndex>>,
    index_task: Option<Task<()>>,
    results: Arc<[VisibleFile]>,
    search_task: Option<Task<()>>,
    watched: Vec<PathBuf>,
    watch: Option<Task<()>>,
}

impl Default for FilesPane {
    fn default() -> Self {
        Self {
            open: false,
            width: DEFAULT_WIDTH,
            resizing: false,
            root: None,
            rows: Arc::default(),
            rows_revision: None,
            scroll: UniformListScrollHandle::new(),
            search: None,
            query: String::new(),
            index: None,
            index_task: None,
            results: Arc::default(),
            search_task: None,
            watched: Vec::new(),
            watch: None,
        }
    }
}

fn clamped_width(width: f32, window_width: f32) -> f32 {
    let max = (window_width / 2.0).clamp(0.0, 720.0);
    width.clamp(200.0_f32.min(max), max)
}

fn directory_stamps(paths: &[PathBuf]) -> Vec<Option<SystemTime>> {
    paths
        .iter()
        .map(|path| path.metadata().and_then(|m| m.modified()).ok())
        .collect()
}

impl Workspace {
    pub(super) fn files_width(&self, viewport: f32) -> f32 {
        if self.files_pane.open {
            clamped_width(self.files_pane.width, viewport)
        } else {
            0.0
        }
    }

    pub(super) fn toggle_files(
        &mut self,
        _: &ToggleFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files_pane.open = !self.files_pane.open;
        if self.files_pane.open {
            self.compact_sidebar_open = false;
            let mut tree = self.file_tree.borrow_mut();
            let cached = tree.rows.keys().cloned().collect::<Vec<_>>();
            tree.dirty.extend(cached);
            drop(tree);
            self.files_pane.index = None;
            self.start_files_search(cx);
        } else {
            let pane = &mut self.files_pane;
            pane.watched.clear();
            pane.watch = None;
            pane.index_task = None;
            pane.search_task = None;
            pane.resizing = false;
            if pane
                .search
                .as_ref()
                .is_some_and(|input| input.read(cx).focus_handle.is_focused(window))
            {
                self.focus_active(window, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn open_files_root(&self, cx: &App) -> Option<PathBuf> {
        self.files_pane.open.then(|| self.files_root(cx)).flatten()
    }

    fn files_root(&self, cx: &App) -> Option<PathBuf> {
        let panel = self
            .slots
            .get(self.active)
            .filter(|slot| !slot.closing)?
            .panel
            .read(cx);
        if harness::is_remote_session(&panel.session_id) {
            return None;
        }
        if panel.session_id.starts_with("file://") {
            return self
                .files_pane
                .root
                .clone()
                .or_else(|| panel.working_dir.as_deref().map(PathBuf::from));
        }
        panel.working_dir.as_deref().map(PathBuf::from)
    }

    fn update_file_watch(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let pane = &mut self.files_pane;
        if pane.watched == paths {
            return;
        }
        pane.watched = paths.clone();
        pane.watch = if paths.is_empty() {
            None
        } else {
            Some(cx.spawn(async move |workspace, cx| {
                // Poll directory metadata, not directory entries. Only paths in the
                // expanded tree are watched, and the task is dropped with the pane.
                let executor = cx.background_executor().clone();
                let mut previous = None;
                loop {
                    let paths_for_stat = paths.clone();
                    let stamps = executor
                        .spawn(async move { directory_stamps(&paths_for_stat) })
                        .await;
                    if let Some(previous) = previous.replace(stamps.clone()) {
                        let changed: Vec<_> = paths
                            .iter()
                            .zip(stamps.iter().zip(&previous))
                            .filter_map(|(path, (new, old))| (new != old).then_some(path.clone()))
                            .collect();
                        if !changed.is_empty()
                            && workspace
                                .update(cx, |workspace, cx| {
                                    workspace.file_tree.borrow_mut().dirty.extend(changed);
                                    workspace.files_pane.index = None;
                                    workspace.files_pane.index_task = None;
                                    if workspace.files_pane.query.trim().is_empty() {
                                        cx.notify();
                                    } else {
                                        workspace.start_files_search(cx);
                                    }
                                })
                                .is_err()
                        {
                            break;
                        }
                    }
                    executor.timer(Duration::from_secs(1)).await;
                }
            }))
        };
    }

    fn collect_visible_files(
        &self,
        directory: &Path,
        depth: usize,
        rows: &mut Vec<VisibleFile>,
        watched: &mut Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        watched.push(directory.to_path_buf());
        for row in self.file_entries(directory, cx).iter() {
            let expanded = row.is_dir && self.expanded_directories.contains(&row.path);
            rows.push(VisibleFile {
                path: row.path.clone(),
                label: row.label.clone(),
                folder: row.is_dir.then_some(expanded),
                depth,
            });
            if expanded {
                self.collect_visible_files(&row.path, depth + 1, rows, watched, cx);
            }
        }
    }

    fn refresh_files(&mut self, cx: &mut Context<Self>) {
        let mut tree = self.file_tree.borrow_mut();
        tree.dirty.extend(self.files_pane.watched.iter().cloned());
        drop(tree);
        self.files_pane.index = None;
        self.files_pane.index_task = None;
        self.start_files_search(cx);
    }

    fn start_files_search(&mut self, cx: &mut Context<Self>) {
        let pane = &mut self.files_pane;
        pane.search_task = None;
        let query = pane.query.trim().to_owned();
        let Some(root) = pane.root.clone().filter(|_| !query.is_empty()) else {
            pane.results = Arc::default();
            cx.notify();
            return;
        };
        let Some(index) = pane.index.clone() else {
            if pane.index_task.is_none() {
                pane.index_task = Some(cx.spawn(async move |workspace, cx| {
                    let index = cx
                        .background_executor()
                        .spawn(async move { Arc::new(FileIndex::scan_for_explorer(&root)) })
                        .await;
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace.files_pane.index = Some(index);
                        workspace.files_pane.index_task = None;
                        workspace.start_files_search(cx);
                    });
                }));
            }
            cx.notify();
            return;
        };
        pane.search_task = Some(cx.spawn(async move |workspace, cx| {
            let results: Arc<[VisibleFile]> = cx
                .background_executor()
                .spawn(async move {
                    index
                        .search_terms(&query, 2000)
                        .into_iter()
                        .map(|path| {
                            let label = path.to_string_lossy().into_owned().into();
                            VisibleFile {
                                path: root.join(path),
                                label,
                                folder: None,
                                depth: 0,
                            }
                        })
                        .collect()
                })
                .await;
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.files_pane.results = results;
                workspace.files_pane.search_task = None;
                workspace.files_pane.scroll = UniformListScrollHandle::new();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn search_field(&mut self, cx: &mut Context<Self>) -> Entity<PromptInput> {
        self.files_pane
            .search
            .get_or_insert_with(|| {
                let owner = cx.weak_entity();
                cx.new(|cx| {
                    PromptInput::embedded_editor(cx, "Find file…")
                        .text_only()
                        .with_on_change(move |query, app| {
                            let _ = owner.update(app, |workspace, cx| {
                                if workspace.files_pane.query != query {
                                    workspace.files_pane.query = query.to_owned();
                                    workspace.files_pane.results = Arc::default();
                                    workspace.start_files_search(cx);
                                }
                            });
                        })
                })
            })
            .clone()
    }

    pub(super) fn render_files_pane(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let root = self.files_root(cx);
        if root != self.files_pane.root {
            let pane = &mut self.files_pane;
            pane.root = root.clone();
            pane.index = None;
            pane.index_task = None;
            pane.results = Arc::default();
            pane.scroll = UniformListScrollHandle::new();
            self.start_files_search(cx);
        }
        if let Some(root) = &root {
            self.set_file_tree_root(root);
            let tree = self.file_tree.borrow();
            let revision = tree.revision;
            let dirty = !tree.dirty.is_empty();
            drop(tree);
            if self.files_pane.rows_revision != Some(revision)
                || dirty
                || self.files_pane.watch.is_none()
            {
                let mut rows = Vec::new();
                let mut watched = Vec::new();
                self.collect_visible_files(root, 0, &mut rows, &mut watched, cx);
                self.files_pane.rows = rows.into();
                self.files_pane.rows_revision = Some(revision);
                self.update_file_watch(watched, cx);
            }
        } else {
            self.files_pane.rows = Arc::default();
            self.files_pane.rows_revision = None;
            self.update_file_watch(Vec::new(), cx);
        }
        let searching = !self.files_pane.query.trim().is_empty();
        let row_count = if searching {
            self.files_pane.results.len()
        } else {
            self.files_pane.rows.len()
        };
        let scroll = self.files_pane.scroll.clone();
        let name = root.as_ref().map(|root| {
            root.file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned()
        });
        let input = root.as_ref().map(|_| self.search_field(cx));
        let root_tooltip: SharedString = root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
            .into();
        let resize = div()
            .id("workspace-files-resize")
            .absolute()
            .left_0()
            .top_0()
            .bottom_0()
            .w(px(5.0))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|workspace, event: &gpui::MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if event.click_count == 2 {
                        workspace.files_pane.width = DEFAULT_WIDTH;
                    } else {
                        workspace.files_pane.resizing = true;
                    }
                    cx.notify();
                }),
            );
        div()
            .id("workspace-files-panel")
            .debug_selector(|| "workspace-files-panel".into())
            .w(px(width))
            .h_full()
            .flex_none()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .occlude()
            .bg(Theme::global().HEADER_BG)
            .border_l_1()
            .border_color(Theme::global().PANEL_BORDER)
            .capture_action(cx.listener(|workspace, _: &crate::input::Clear, _, cx| {
                if !workspace.files_pane.query.is_empty()
                    && let Some(input) = workspace.files_pane.search.clone()
                {
                    input.update(cx, |input, cx| input.set_content(String::new(), cx));
                    workspace.files_pane.query.clear();
                    workspace.start_files_search(cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(
                cx.listener(|workspace, _: &crate::input::Submit, window, cx| {
                    cx.stop_propagation();
                    let pane = &workspace.files_pane;
                    if pane.index_task.is_none()
                        && pane.search_task.is_none()
                        && let Some(path) = pane.results.first().map(|row| row.path.clone())
                    {
                        workspace.open_code_file(path, window, cx);
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_3()
                    .pr_1()
                    .h(px(36.0))
                    .flex_none()
                    .child(
                        div()
                            .id("workspace-files-root")
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .tooltip(move |_, cx| {
                                cx.new(|_| super::remotes::HeaderTooltip(root_tooltip.clone()))
                                    .into()
                            })
                            .child(name.unwrap_or_else(|| "Files".into())),
                    )
                    .when(root.is_some(), |header| {
                        header
                            .child(self.files_button(
                                "workspace-files-collapse",
                                "Collapse folders",
                                "−",
                                |workspace, _, cx| {
                                    workspace.expanded_directories.clear();
                                    let mut tree = workspace.file_tree.borrow_mut();
                                    let root = tree.root.clone();
                                    tree.rows.retain(|path, _| Some(path) == root.as_ref());
                                    tree.dirty.retain(|path| Some(path) == root.as_ref());
                                    tree.revision += 1;
                                    cx.notify();
                                },
                                cx,
                            ))
                            .child(self.files_button(
                                "workspace-files-refresh",
                                "Refresh files",
                                "↻",
                                |workspace, _, cx| workspace.refresh_files(cx),
                                cx,
                            ))
                    })
                    .child(self.files_button(
                        "workspace-files-close",
                        "Hide Files",
                        "×",
                        |workspace, window, cx| workspace.toggle_files(&ToggleFiles, window, cx),
                        cx,
                    )),
            )
            .when_some(input, |pane, input| {
                pane.child(
                    div()
                        .rounded_full()
                        .bg(Theme::global().PANEL_BG)
                        .debug_selector(|| "workspace-files-search".into())
                        .px_2()
                        .pt_1()
                        .pb_2()
                        .child(input),
                )
            })
            .when(row_count > 0, |pane| {
                pane.child(
                    gpui::uniform_list(
                        "workspace-files-list",
                        row_count,
                        cx.processor(move |workspace, range: std::ops::Range<usize>, _, cx| {
                            let rows = if searching {
                                &workspace.files_pane.results
                            } else {
                                &workspace.files_pane.rows
                            };
                            range
                                .filter_map(|index| rows.get(index))
                                .map(|row| workspace.render_visible_file(row, cx))
                                .collect()
                        }),
                    )
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&scroll),
                )
            })
            .when(row_count == 0, |pane| {
                pane.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(px(12.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(if root.is_none() {
                            "No local working directory"
                        } else if searching
                            && (self.files_pane.index_task.is_some()
                                || self.files_pane.search_task.is_some())
                        {
                            "Searching…"
                        } else if searching {
                            "No matching files"
                        } else {
                            "No files to show"
                        }),
                )
            })
            .child(resize)
            .into_any_element()
    }

    fn render_visible_file(
        &self,
        row: &VisibleFile,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let selector = format!("right-file-tree-{}", row.label);
        let icon = match row.folder {
            Some(true) => "▾",
            Some(false) => "▸",
            None => match row.path.extension().and_then(|ext| ext.to_str()) {
                Some("rs" | "c" | "h" | "py" | "js" | "ts" | "go" | "sh") => "‹›",
                Some("toml" | "json" | "yml" | "yaml") => "⚙",
                _ => "▤",
            },
        };
        let path = row.path.clone();
        let folder = row.folder.is_some();
        div()
            .id(SharedString::from(format!(
                "right-file-tree:{}",
                row.path.display()
            )))
            .relative()
            .children(crate::fixture_control_targets::marker(
                {
                    let selector = selector.clone();
                    move || selector
                },
                || None,
            ))
            .debug_selector(move || selector.clone())
            .h(px(25.0))
            .w_full()
            .pl(px(10.0 + 14.0 * row.depth.min(20) as f32))
            .pr_2()
            .rounded_full()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_size(px(12.0))
            .hover(|el| el.bg(Theme::global().PANEL_BG))
            .on_click(cx.listener(move |workspace, _, window, cx| {
                if folder {
                    workspace.toggle_file_directory(&path);
                    cx.notify();
                } else {
                    workspace.open_code_file(path.clone(), window, cx);
                }
            }))
            .child(
                div()
                    .w(px(14.0))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(Theme::global().TEXT_DIM)
                    .child(icon),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(row.label.clone()),
            )
    }

    pub(super) fn files_button(
        &self,
        id: &'static str,
        title: &'static str,
        label: &'static str,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let owner = cx.weak_entity();
        crate::keyboard::press_button(div().id(id), move |window, cx| {
            cx.stop_propagation();
            window.prevent_default();
            let _ = owner.update(cx, |workspace, cx| action(workspace, window, cx));
        })
        .debug_selector(move || id.into())
        .children(crate::fixture_control_targets::marker(
            move || id.into(),
            || None,
        ))
        .size(px(27.0))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .occlude()
        .text_color(Theme::global().TEXT_DIM)
        .hover(|el| {
            el.bg(Theme::global().PANEL_BG)
                .text_color(Theme::global().TEXT)
        })
        .tooltip(move |_, cx| {
            cx.new(|_| super::remotes::HeaderTooltip(title.into()))
                .into()
        })
        .child(label)
    }

    pub(super) fn render_files_resize_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("workspace-files-resize-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .on_mouse_move(
                cx.listener(|workspace, event: &gpui::MouseMoveEvent, window, cx| {
                    if event.dragging() {
                        let width = f32::from(window.viewport_size().width);
                        workspace.files_pane.width =
                            clamped_width(width - f32::from(event.position.x), width);
                    } else {
                        workspace.files_pane.resizing = false;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|workspace, _, _, cx| {
                    workspace.files_pane.resizing = false;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|workspace, _, _, cx| {
                    workspace.files_pane.resizing = false;
                    cx.notify();
                }),
            )
            .into_any_element()
    }
}
