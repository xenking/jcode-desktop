//! Jcode's own directory browser, as a view.
//!
//! One picker serves two surfaces. Opening a folder shows it in the
//! [`ModalLayer`](crate::modal_layer::ModalLayer). Choosing the default
//! directory shows the same view inline, inside its own panel, so the
//! workspace holds that one directly and the panel renders it.
use super::*;
use gpui::DismissEvent;

#[derive(PartialEq, Eq)]
struct FolderSearchKey {
    directory: PathBuf,
    query: String,
    sets_default: bool,
}

type FolderMatches = std::sync::Arc<[(PathBuf, String)]>;

#[derive(Default)]
struct FolderCandidates {
    result: Option<(FolderSearchKey, FolderMatches)>,
    running: bool,
}

pub(super) struct FolderPicker {
    workspace: gpui::WeakEntity<Workspace>,
    /// Directory being browsed.
    pub(super) dir: PathBuf,
    pub(super) error: Option<String>,
    pub(super) search: Entity<PromptInput>,
    /// Choosing the default directory instead of opening a session.
    pub(super) sets_default: bool,
    /// The panel whose folder is being switched, when opened from its pill.
    pub(super) source: Option<gpui::EntityId>,
    candidates: FolderCandidates,
}

impl FolderPicker {
    pub(super) fn new(
        workspace: gpui::WeakEntity<Workspace>,
        sets_default: bool,
        dir: PathBuf,
        source: Option<gpui::EntityId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        let change = weak.clone();
        let cancel = weak.clone();
        let search = cx.new(|cx| {
            PromptInput::new(
                cx,
                if sets_default {
                    "enter a directory path, then press enter to set default"
                } else {
                    "type a folder name or path, then press enter"
                },
                move |query, _, _, app| {
                    let _ = weak.update(app, |this, cx| this.open_searched(&query, cx));
                },
            )
            .with_on_overlay_cancel(move |app| {
                let _ = cancel.update(app, |_, cx| cx.emit(DismissEvent));
                true
            })
            .with_on_change(move |_, app| {
                let _ = change.update(app, |_, cx| cx.notify());
            })
        });
        Self {
            workspace,
            dir,
            error: None,
            search,
            sets_default,
            source,
            candidates: FolderCandidates::default(),
        }
    }

    fn home(&self, cx: &App) -> Option<PathBuf> {
        self.workspace
            .read_with(cx, |workspace, _| workspace.default_working_dir())
            .ok()
            .flatten()
            .map(PathBuf::from)
    }

    fn open_searched(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.sets_default {
            self.set_searched_default(query, cx);
            return;
        }
        let home = self.home(cx);
        let base = self.dir.clone();
        let expanded = if query == "~" {
            home
        } else if let Some(rest) = query.strip_prefix("~/") {
            home.map(|home| home.join(rest))
        } else {
            let path = PathBuf::from(query);
            Some(if path.is_absolute() {
                path
            } else {
                base.join(path)
            })
        };
        let matched = expanded.filter(|path| path.is_dir()).or_else(|| {
            let needle = query.to_lowercase();
            let direct = base.join(query);
            if direct.is_file() {
                return direct.parent().map(Path::to_path_buf);
            }
            let sessions = self
                .workspace
                .read_with(cx, |workspace, _| workspace.sessions.clone())
                .unwrap_or_default();
            ranked_folder_matches_for_sessions(&sessions, &base, &needle)
                .into_iter()
                .next()
                .map(|(path, _)| path)
        });
        if let Some(path) = matched {
            self.dir = path;
            self.choose(cx);
        } else {
            self.error = Some(format!("no folder matches ‘{query}’"));
            cx.notify();
        }
    }

    pub(super) fn browse_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match directory_entries(&path) {
            Ok(_) => {
                self.dir = path;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    /// Open the browsed folder, or save it as the default directory.
    pub(super) fn choose(&mut self, cx: &mut Context<Self>) {
        if self.sets_default {
            self.set_default(self.dir.clone(), cx);
            return;
        }
        let directory = self.dir.to_string_lossy().into_owned();
        let source = self.source.take();
        cx.emit(Chosen { directory, source });
        cx.emit(DismissEvent);
    }

    pub(super) fn set_searched_default(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim();
        let home = self.home(cx);
        let path = if query == "~" {
            home
        } else if let Some(rest) = query.strip_prefix("~/") {
            home.map(|home| home.join(rest))
        } else {
            let path = PathBuf::from(query);
            if path.is_absolute() {
                Some(path)
            } else {
                Some(self.dir.join(path))
            }
        };
        if let Some(path) = path {
            self.set_default(path, cx);
        } else {
            self.error = Some("Enter an existing directory path.".into());
            cx.notify();
        }
    }

    fn set_default(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // Never silently substitute a fuzzy match or a file's parent for a preference.
        let result = (|| -> Result<String, String> {
            if !path.is_absolute() || !path.is_dir() {
                return Err(format!("Not an existing directory: {}", path.display()));
            }
            std::fs::read_dir(&path)
                .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
            let directory = path
                .to_str()
                .ok_or_else(|| "Directory path is not valid UTF-8.".to_string())?
                .to_owned();
            crate::config::persist_pinned_working_dir(&directory)
                .map_err(|error| format!("Could not save default directory: {error}"))?;
            Ok(directory)
        })();
        match result {
            Ok(directory) => {
                let _ = self.workspace.update(cx, |workspace, _| {
                    workspace.pinned_working_dir = Some(directory);
                });
                cx.emit(DismissEvent);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn matches(&mut self, query: String, cx: &mut Context<Self>) -> FolderMatches {
        let key = FolderSearchKey {
            directory: self.dir.clone(),
            query,
            sets_default: self.sets_default,
        };
        if let Some((previous, candidates)) = &self.candidates.result
            && previous == &key
        {
            return candidates.clone();
        }
        if self.candidates.running {
            return Default::default();
        }
        self.candidates.running = true;
        let sessions = self
            .workspace
            .read_with(cx, |workspace, _| workspace.sessions.clone())
            .unwrap_or_default();
        let load = cx.background_spawn(async move {
            let candidates = if key.sets_default {
                ranked_directories(&sessions, &key.directory, &key.query)
            } else {
                ranked_folder_matches_for_sessions(&sessions, &key.directory, &key.query)
            };
            (key, candidates.into())
        });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                this.candidates.result = Some(result);
                this.candidates.running = false;
                cx.notify();
            });
        })
        .detach();
        Default::default()
    }
}

/// The user opened a folder in the picker. The picker only reports the
/// choice. What opening means belongs to whoever showed it.
pub(super) struct Chosen {
    pub(super) directory: String,
    /// The panel whose folder the choice replaces, when opened from its pill.
    pub(super) source: Option<gpui::EntityId>,
}

impl gpui::EventEmitter<Chosen> for FolderPicker {}
impl gpui::EventEmitter<DismissEvent> for FolderPicker {}
impl ModalView for FolderPicker {}

impl Focusable for FolderPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.read(cx).focus_handle.clone()
    }
}

impl Workspace {
    /// The picker on screen, whichever surface shows it.
    pub(super) fn folder_picker(&self, cx: &App) -> Option<Entity<FolderPicker>> {
        self.default_directory_picker
            .clone()
            .or_else(|| self.active_modal::<FolderPicker>(cx))
    }

}

impl Render for FolderPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let directory = self.dir.clone();
        let query = self.search.read(cx).content.trim().to_lowercase();
        let entries = self.matches(query, cx);
        let (pinned, description) = self
            .workspace
            .read_with(cx, |workspace, _| {
                (
                    workspace.pinned_working_dir.clone(),
                    self.sets_default
                        .then(|| workspace.default_directory_description(&directory)),
                )
            })
            .unwrap_or_default();
        let description = description.unwrap_or_default();
        let home = crate::platform_dirs::home_string();
        let mut list = div()
            .id("folder-picker-list")
            .debug_selector(|| "folder-picker-list".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .border_t_1()
            .border_b_1()
            .border_color(Theme::global().PANEL_BORDER);

        if let Some(parent) = directory.parent().map(Path::to_path_buf) {
            list = list.child(
                div()
                    .id("folder-picker-parent")
                    .debug_selector(|| "folder-picker-parent".into())
                    .px_4()
                    .py_2()
                    .cursor_pointer()
                    .text_color(Theme::global().TEXT_DIM)
                    .hover(|el| {
                        el.bg(Theme::global().HEADER_BG)
                            .text_color(Theme::global().TEXT)
                    })
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.browse_to(parent.clone(), cx);
                        }),
                    )
                    .child("↰  .."),
            );
        }
        for (index, (path, reason)) in entries.iter().cloned().enumerate() {
            let label = if self.sets_default {
                compact_path_in(&path.to_string_lossy(), home.clone())
            } else {
                path.strip_prefix(&directory)
                    .ok()
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.display().to_string())
                    .unwrap_or_else(|| path.display().to_string())
            };
            let sets_default = self.sets_default;
            let reason = if sets_default && pinned.as_deref() == path.to_str() {
                "current default".into()
            } else {
                reason
            };
            list = list.child(
                div()
                    .id(("folder-picker-entry", index))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || format!("folder-picker-entry-{index}"),
                        || None,
                    ))
                    .debug_selector(move || format!("folder-picker-entry-{index}"))
                    .px_4()
                    .py_2()
                    .cursor_pointer()
                    .hover(|el| el.bg(Theme::global().HEADER_BG))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        cx.stop_propagation();
                        if sets_default {
                            this.set_default(path.clone(), cx);
                        } else {
                            this.browse_to(path.clone(), cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("▸  {label}")),
                            )
                            .when(!reason.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(Theme::global().TEXT_DIM)
                                        .child(reason),
                                )
                            }),
                    ),
            );
        }

        let inline = self.sets_default;
        div()
            .id(if inline {
                "default-directory-panel"
            } else {
                "folder-picker-overlay"
            })
            .debug_selector(move || {
                if inline {
                    "default-directory-panel".into()
                } else {
                    "folder-picker-overlay".into()
                }
            })
            .key_context(crate::menu::CONTEXT)
            .on_action(cx.listener(|_, _: &crate::menu::Cancel, _, cx| cx.emit(DismissEvent)))
            // Clicking a modal control must not let the workspace ancestor
            // steal the search focus. In particular, a rejected save needs to
            // keep accepting edits and Escape without an extra input click.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    let sets_default = this.sets_default;
                    let _ = this.workspace.update(cx, |workspace, cx| {
                        if sets_default
                            && let Some(index) = workspace.default_directory_panel_index(cx)
                        {
                            workspace.set_active(index, cx);
                        }
                        workspace.focus_pending = false;
                    });
                    window.focus(&this.search.read(cx).focus_handle.clone(), cx);
                }),
            )
            .when(inline, |el| el.size_full())
            .when(!inline, |el| {
                el.absolute()
                    .inset_0()
                    .occlude()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgba(0x000000cc))
            })
            .flex()
            .child(
                div()
                    .when(inline, |el| el.size_full().min_w_0())
                    .when(!inline, |el| {
                        el.w(px(680.0))
                            .h(px(560.0))
                            .max_w(relative(0.9))
                            .max_h(relative(0.85))
                    })
                    .flex()
                    .flex_col()
                    .when(!inline, |el| {
                        el.rounded_lg()
                            .border_1()
                            .border_color(Theme::global().PANEL_BORDER_FOCUS)
                            .bg(Theme::global().PANEL_BG)
                    })
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(px(14.0)).child(
                                if self.sets_default {
                                    "Default directory"
                                } else if self.source.is_some() {
                                    "switch folder"
                                } else {
                                    "open folder"
                                },
                            ))
                            .child(
                                div()
                                    .id("folder-picker-cancel")
                                    .debug_selector(|| "folder-picker-cancel".into())
                                    .cursor_pointer()
                                    .text_color(Theme::global().TEXT_DIM)
                                    .hover(|el| el.text_color(Theme::global().TEXT))
                                    .on_click(cx.listener(|_, _event, _window, cx| {
                                        cx.stop_propagation();
                                        cx.emit(DismissEvent);
                                    }))
                                    .child("cancel"),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .child(if self.sets_default {
                                description
                            } else {
                                directory.display().to_string()
                            }),
                    )
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("folder-picker-home")
                                    .debug_selector(|| "folder-picker-home".into())
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(Theme::global().HEADER_BG)
                                    .hover(|el| el.text_color(Theme::global().TEXT))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|this, _event, _window, cx| {
                                            if let Some(home) = this.home(cx) {
                                                this.browse_to(home, cx);
                                            }
                                        }),
                                    )
                                    .child("⌂  home"),
                            )
                            .child(
                                div()
                                    .id("folder-picker-computer")
                                    .debug_selector(|| "folder-picker-computer".into())
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(Theme::global().HEADER_BG)
                                    .hover(|el| el.text_color(Theme::global().TEXT))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|this, _event, _window, cx| {
                                            this.browse_to(filesystem_root(), cx);
                                        }),
                                    )
                                    .child("▣  computer"),
                            ),
                    )
                    .child(
                        div()
                            .id("folder-picker-search")
                            .debug_selector(|| "folder-picker-search".into())
                            .mx_4()
                            .mb_3()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(Theme::global().INPUT_BORDER)
                            .bg(Theme::global().INPUT_BG)
                            .child(self.search.clone()),
                    )
                    .child(list)
                    .when_some(self.error.clone(), |el, error| {
                        el.child(
                            div()
                                .debug_selector(|| "folder-picker-error".into())
                                .px_4()
                                .py_2()
                                .text_color(Theme::global().ERROR)
                                .child(error),
                        )
                    })
                    .child(
                        div().p_3().flex().justify_end().child(
                            div()
                                .id("folder-picker-open")
                                .relative()
                                .children(crate::fixture_control_targets::marker(
                                    || "folder-picker-open".into(),
                                    || None,
                                ))
                                .debug_selector(|| "folder-picker-open".into())
                                .px_4()
                                .py_2()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(Theme::global().ACCENT)
                                .text_color(Theme::global().BG)
                                // Keep the modal mounted until release. Opening on
                                // mouse-down lets the rest of the click hit the
                                // newly repositioned strip and steal draft focus.
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    let query = this.search.read(cx).content.trim().to_string();
                                    if this.sets_default && !query.is_empty() {
                                        this.set_searched_default(&query, cx);
                                    } else {
                                        this.choose(cx);
                                    }
                                }))
                                .child(if self.sets_default {
                                    "Set as default"
                                } else {
                                    "open this folder"
                                }),
                        ),
                    ),
            )
    }
}

/// Read-only views of the picker for tests, shaped like the fields they replaced.
#[cfg(test)]
impl Workspace {
    pub(super) fn picker_sets_default(&self, cx: &App) -> bool {
        self.folder_picker(cx)
            .is_some_and(|picker| picker.read(cx).sets_default)
    }

    pub(super) fn picker_dir(&self, cx: &App) -> Option<PathBuf> {
        self.folder_picker(cx).map(|picker| picker.read(cx).dir.clone())
    }

    pub(super) fn picker_error(&self, cx: &App) -> Option<String> {
        self.folder_picker(cx)
            .and_then(|picker| picker.read(cx).error.clone())
    }

    pub(super) fn picker_search(&self, cx: &App) -> Option<Entity<PromptInput>> {
        self.folder_picker(cx)
            .map(|picker| picker.read(cx).search.clone())
    }

    /// Edit the open picker in place, as if the user had browsed there.
    pub(super) fn with_picker(&self, cx: &mut App, edit: impl FnOnce(&mut FolderPicker)) {
        let picker = self.folder_picker(cx).expect("a picker is open");
        picker.update(cx, |picker, _| edit(picker));
    }

    pub(super) fn browse_picker_to(&self, path: PathBuf, cx: &mut App) {
        let picker = self.folder_picker(cx).expect("a picker is open");
        picker.update(cx, |picker, cx| picker.browse_to(path, cx));
    }

    pub(super) fn picker_source(&self, cx: &App) -> Option<gpui::EntityId> {
        self.folder_picker(cx).and_then(|picker| picker.read(cx).source)
    }
}

impl Workspace {
    pub(super) fn default_directory_description(&self, browsing: &Path) -> String {
        let current = self
            .pinned_working_dir
            .clone()
            .or_else(|| self.default_working_dir())
            .unwrap_or_else(|| "Not set".into());
        let scope = match &self.remotes.default_host {
            Some(host) => format!(
                "{} currently uses {host}:~. This preference applies to This computer.",
                spawn_shortcut()
            ),
            None => format!("{} opens in {current}", spawn_shortcut()),
        };
        format!(
            "{scope}\nChoose a frequent folder or enter a path. Browsing: {}",
            browsing.display()
        )
    }
}

pub(super) fn spawn_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Enter"
    } else {
        "Super+Enter"
    }
}

#[cfg(test)]
pub(super) fn compact_path(path: &str) -> String {
    compact_path_in(path, default_working_dir())
}

pub(super) fn compact_path_in(path: &str, default_dir: Option<String>) -> String {
    if let Some(home) = default_dir {
        if path == home {
            return "~ (home)".into();
        }
        if let Ok(relative) = Path::new(path).strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    path.into()
}

/// Rank local session history by use count, then recency, with stable path ties.
/// The current directory and its children remain available even with no history.
pub(super) fn ranked_directories(
    sessions: &[jcode_sdk::SessionInfo],
    base: &Path,
    query: &str,
) -> Vec<(PathBuf, String)> {
    let mut usage = HashMap::<PathBuf, (usize, usize)>::new();
    for (recency, session) in sessions.iter().rev().enumerate() {
        if harness::is_remote_session(&session.session_id) {
            continue;
        }
        let Some(path) = session.working_dir.as_deref().map(PathBuf::from) else {
            continue;
        };
        if path.is_absolute() && path.is_dir() {
            let entry = usage.entry(path).or_insert((0, recency));
            entry.0 += 1;
        }
    }
    for path in
        std::iter::once(base.to_path_buf()).chain(directory_entries(base).unwrap_or_default())
    {
        if path.is_absolute() && path.is_dir() {
            usage.entry(path).or_insert((0, usize::MAX));
        }
    }
    let mut entries: Vec<_> = usage
        .into_iter()
        .filter(|(path, _)| path.to_string_lossy().to_lowercase().contains(query))
        .collect();
    entries.sort_by(|(a, (ac, ar)), (b, (bc, br))| bc.cmp(ac).then(ar.cmp(br)).then(a.cmp(b)));
    entries
        .into_iter()
        .map(|(path, (count, _))| {
            (
                path,
                match count {
                    0 => "Set as default".into(),
                    1 => "1 session · Set as default".into(),
                    _ => format!("{count} sessions · Set as default"),
                },
            )
        })
        .collect()
}
