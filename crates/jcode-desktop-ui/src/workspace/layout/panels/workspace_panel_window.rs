//! Native utility windows for standalone mode. The originating workspace is never rearranged.
use super::*;
use gpui::{AnyWindowHandle, WeakEntity, WindowOptions};

/// Open a terminal in its own native window, or activate the open one.
pub(crate) fn open_terminal_window(
    working_dir: Option<String>,
    host: HostHandle,
    cx: &mut App,
) -> anyhow::Result<()> {
    if let Some(handle) = cx.windows().into_iter().find_map(|handle| {
        let handle = handle.downcast::<PanelWindow>()?;
        handle
            .read(cx)
            .is_ok_and(PanelWindow::is_terminal)
            .then_some(handle)
    }) {
        return handle.update(cx, |root, window, cx| {
            root.activate(window, cx);
        });
    }
    cx.open_window(
        WindowOptions {
            app_id: Some(crate::APP_ID.into()),
            focus: true,
            ..Default::default()
        },
        move |window, cx| {
            window.set_window_title("terminal · Jcode");
            window.activate_window();
            let terminal = cx.new(|cx| TerminalPanel::new(working_dir, None, None, host, cx));
            let item = PanelItem::Terminal(terminal);
            item.surface().focus_input(window, cx);
            let origin = window.window_handle();
            cx.new(|_| PanelWindow::new(item, None, None, origin))
        },
    )?;
    Ok(())
}

type RestoreOrigin = fn(Option<&str>, Option<Entity<Panel>>, bool, bool, &mut Window, &mut App) -> bool;
pub(super) struct OriginRoutes {
    pub(super) restore: RestoreOrigin,
    pub(super) fleet: fn(&crate::ui_actions::OpenFleetSession, &mut Window, &mut App),
}
impl gpui::Global for OriginRoutes {}

pub(crate) struct PanelWindow {
    item: PanelItem,
    source: Option<WeakEntity<Panel>>,
    source_session: Option<String>,
    source_window: AnyWindowHandle,
    finished: bool,
    window_background: Option<gpui::WindowBackgroundAppearance>,
}

/// Open a utility panel in its own native window, or activate its existing window.
/// Return the hosted entity so callers can apply commands even when reusing a window.
pub(crate) fn open_panel_window(
    panel: Entity<Panel>,
    source: Option<Entity<Panel>>,
    source_window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<Entity<Panel>> {
    open_panel_window_at(panel, source, source_window.window_handle(), true, cx)
}

/// Event-driven documents may open without a borrowed window or focus intent.
pub(crate) fn open_panel_window_at(
    panel: Entity<Panel>,
    source: Option<Entity<Panel>>,
    origin: AnyWindowHandle,
    activate: bool,
    cx: &mut App,
) -> anyhow::Result<Entity<Panel>> {
    open_item_window(PanelItem::Chat(panel), source, origin, activate, cx)?
        .chat()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("the window for this session hosts another kind"))
}

/// Open a file in its own native window, or activate the window already showing it.
pub(crate) fn open_file_window(
    file: Entity<FilePanel>,
    source_window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<Entity<FilePanel>> {
    open_item_window(
        PanelItem::File(file),
        None,
        source_window.window_handle(),
        true,
        cx,
    )?
    .profile().file()
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("the window for this file hosts another kind"))
}

pub(crate) fn open_item_window(
    item: PanelItem,
    source: Option<Entity<Panel>>,
    origin: AnyWindowHandle,
    activate: bool,
    cx: &mut App,
) -> anyhow::Result<PanelItem> {
    let session_id = item.session_id(cx).to_owned();
    let source_session = source
        .as_ref()
        .map(|source| source.read(cx).session_id.clone());
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<PanelWindow>() else {
            continue;
        };
        let Some(existing) = handle.read(cx).ok().and_then(|root| {
            (!root.finished && root.item.session_id(cx) == session_id).then(|| root.item.clone())
        }) else {
            continue;
        };
        return handle.update(cx, |root, window, cx| {
            root.source = source.as_ref().map(Entity::downgrade);
            root.source_session = source_session;
            root.source_window = origin;
            if activate {
                root.item.surface().focus_input(window, cx);
                window.activate_window();
            }
            existing
        });
    }
    let hosted = item.clone();
    cx.open_window(
        WindowOptions {
            app_id: Some(crate::APP_ID.into()),
            focus: activate,
            ..Default::default()
        },
        move |window, cx| {
            window.set_window_title(&format!("{} · Jcode", item.profile().title(cx)));
            if activate {
                window.activate_window();
            }
            cx.new(|cx| {
                if let Some(panel) = item.chat() {
                    cx.subscribe_in(
                        panel,
                        window,
                        |this: &mut PanelWindow,
                         _,
                         _: &crate::panel::AccountsPanelClosed,
                         window,
                         cx| {
                            this.close(false, window, cx);
                        },
                    )
                    .detach();
                    cx.subscribe_in(
                        panel,
                        window,
                        |this: &mut PanelWindow,
                         _,
                         _: &crate::panel::AccountsPanelChooseModel,
                         window,
                         cx| {
                            this.close(true, window, cx);
                        },
                    )
                    .detach();
                }
                let weak = cx.entity().downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    weak.update(cx, |this, cx| {
                        if !this.confirm_close(cx) {
                            return false;
                        }
                        this.finish(false, cx);
                        true
                    })
                    .unwrap_or(true)
                });
                item.surface().focus_input(window, cx);
                PanelWindow::new(
                    item,
                    source.as_ref().map(Entity::downgrade),
                    source_session,
                    origin,
                )
            })
        },
    )?;
    Ok(hosted)
}

impl PanelWindow {
    fn new(
        item: PanelItem,
        source: Option<WeakEntity<Panel>>,
        source_session: Option<String>,
        source_window: AnyWindowHandle,
    ) -> Self {
        Self {
            item,
            source,
            source_session,
            source_window,
            finished: false,
            window_background: None,
        }
    }

    /// The conversation-shaped panel this window hosts, when it hosts one.
    pub(crate) fn chat(&self) -> Option<&Entity<Panel>> {
        self.item.chat()
    }

    #[cfg(test)]
    pub(super) fn hosted_item(&self) -> &PanelItem {
        &self.item
    }

    pub(super) fn is_chat(&self, session: &str, cx: &App) -> bool {
        self.chat().is_some_and(|chat| chat.read(cx).session_id == session)
    }

    fn is_terminal(&self) -> bool {
        self.item.profile().terminal().is_some()
    }

    fn activate(&self, window: &mut Window, cx: &mut App) {
        self.item.surface().focus_input(window, cx);
        window.activate_window();
    }

    /// Unsaved edits may veto the close. Other kinds never do.
    fn confirm_close(&self, cx: &mut App) -> bool {
        self.item.surface().confirm_close(cx)
    }

    /// An agent deletion must not activate the source of a background document.
    pub(super) fn remove_document(&mut self, window: &mut Window) {
        self.finished = true;
        window.remove_window();
    }

    fn finish(&mut self, choose_model: bool, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        let source = self.source.clone();
        let source_session = self.source_session.clone();
        let origin = self.source_window;
        let accounts = self
            .item
            .read_chat(cx)
            .is_some_and(Panel::is_accounts_panel);
        // Release the child window update before focusing its originating window.
        cx.defer(move |cx| {
            let _ = origin.update(cx, |_, window, cx| {
                let fallback = source.and_then(|source| source.upgrade());
                if let Some(restore) = cx.try_global::<OriginRoutes>().map(|routes| routes.restore)
                    && restore(source_session.as_deref(), fallback.clone(), accounts, choose_model, window, cx)
                {
                    return;
                }
                if let Some(source) = fallback {
                    source.update(cx, |panel, cx| panel.focus_input(window, cx));
                    window.activate_window();
                }
            });
        });
    }

    fn close(&mut self, choose_model: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.confirm_close(cx) {
            return;
        }
        self.finish(choose_model, cx);
        window.remove_window();
    }
}

impl Render for PanelWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("PanelWindow");
        Theme::sync_window_background(window, &mut self.window_background);
        let theme = Theme::global();
        let width = window.viewport_size().width;
        self.item.update_chat(cx, |panel, cx| {
            if panel.openrig_width_changed(width, cx) {
                panel.set_openrig_width(width, cx);
            }
        });
        // A terminal owns Escape and its own close, so it never dismisses here.
        let dismissible = self.item.profile().terminal().is_none();
        div()
            .debug_selector(|| "panel-window-root".into())
            .size_full()
            .relative()
            .bg(theme.PANEL_BG)
            .text_color(theme.TEXT)
            .font_family(theme.FONT_UI)
            .text_size(px(14.0 * crate::config::get().appearance.text_scale))
            .on_action(cx.listener(
                |this, action: &crate::ui_actions::OpenFleetSession, _, cx| {
                    let origin = this.source_window;
                    let action = action.clone();
                    cx.defer(move |cx| {
                        let _ = origin.update(cx, |_, window, cx| {
                            if let Some(fleet) = cx.try_global::<OriginRoutes>().map(|routes| routes.fleet) {
                                fleet(&action, window, cx);
                            }
                        });
                    });
                },
            ))
            .on_action(cx.listener(
                |this, action: &change_review::CloseChangeReview, window, cx| {
                    if action.panel == this.item.entity_id() {
                        this.close(false, window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .on_action(cx.listener(|this, _: &ClosePanel, window, cx| {
                if this.item.profile().terminal().is_some() {
                    cx.propagate();
                    return;
                }
                this.close(false, window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(
                |_, action: &crate::ui_actions::OpenDocumentFile, window, cx| {
                    let path = action
                        .path
                        .canonicalize()
                        .unwrap_or_else(|_| action.path.clone());
                    let file = cx.new(|cx| FilePanel::new(path, cx));
                    match open_file_window(file, window, cx) {
                        Ok(file) => {
                            if let Some(fragment) = &action.fragment {
                                file.update(cx, |file, cx| file.show_heading(fragment, cx));
                            }
                        }
                        Err(error) => eprintln!("Could not open panel window: {error:#}"),
                    }
                    cx.stop_propagation();
                },
            ))
            .when(dismissible, |root| {
                root.key_context(crate::menu::CONTEXT)
                    .on_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                        this.close(false, window, cx);
                    }))
            })
            .child(self.item.surface().view())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn escape_closes_the_panel_window(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let (window_view, vcx) = cx.add_window_view(|window, cx| {
            let file = cx.new(|cx| FilePanel::new(path, cx));
            PanelWindow::new(PanelItem::File(file), None, None, window.window_handle())
        });
        vcx.run_until_parked();
        let item = window_view.read_with(vcx, |view, _| view.item.clone());
        vcx.update(|window, cx| item.surface().focus_input(window, cx));
        vcx.simulate_keystrokes("escape");
        assert!(window_view.read_with(vcx, |view, _| view.finished));
    }

    /// The standalone terminal window used to host a bare `TerminalPanel`,
    /// which never asked the compositor for the palette's window background.
    #[gpui::test]
    fn terminal_window_syncs_its_background_with_the_theme(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        cx.update(|cx| open_terminal_window(None, HostHandle::inert(), cx).unwrap());
        cx.run_until_parked();
        let window = cx.update(|cx| {
            let windows = cx.windows();
            assert_eq!(windows.len(), 1);
            windows[0].downcast::<PanelWindow>()
        });
        let window = window.expect("terminal window is a PanelWindow");
        window
            .update(cx, |root, _, _| {
                assert!(root.item.profile().terminal().is_some());
                assert_eq!(root.window_background, Some(Theme::window_background()));
            })
            .unwrap();
    }

    #[gpui::test]
    fn opening_a_file_twice_reuses_its_window(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let (_, vcx) = cx.add_window_view(|_, _| gpui::Empty);
        let (first, second) = vcx.update(|window, cx| {
            let open = |window: &mut Window, cx: &mut App| {
                let file = cx.new(|cx| FilePanel::new(path.clone(), cx));
                open_file_window(file, window, cx).unwrap()
            };
            (open(window, cx), open(window, cx))
        });
        vcx.run_until_parked();
        assert_eq!(first, second);
        vcx.update(|_, cx| {
            assert_eq!(cx.windows().len(), 2);
            assert_eq!(
                first.read(cx).session_id(),
                format!("file://{}", path.display())
            );
        });
    }

    #[gpui::test]
    fn terminal_window_leaves_escape_to_the_shell(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (window_view, vcx) = cx.add_window_view(|window, cx| {
            let terminal =
                cx.new(|cx| TerminalPanel::new(None, None, None, HostHandle::inert(), cx));
            PanelWindow::new(
                PanelItem::Terminal(terminal),
                None,
                None,
                window.window_handle(),
            )
        });
        vcx.run_until_parked();
        let item = window_view.read_with(vcx, |view, _| view.item.clone());
        vcx.update(|window, cx| item.surface().focus_input(window, cx));
        vcx.simulate_keystrokes("escape");
        assert!(!window_view.read_with(vcx, |view, _| view.finished));
    }

    #[gpui::test]
    fn accounts_return_resolves_reloaded_source_by_session(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.single_panel = true;
            w.push_test_panel("reload-source", cx);
            w
        });
        let old_source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
        workspace.update_in(vcx, |w, window, cx| {
            w.open_accounts(
                &OpenAccounts {
                    source: old_source.entity_id(),
                    login_command: None,
                },
                window,
                cx,
            );
        });
        vcx.run_until_parked();
        let (accounts_window, accounts) = vcx.update(|_, cx| {
            cx.windows()
                .into_iter()
                .find_map(|handle| {
                    let handle = handle.downcast::<PanelWindow>()?;
                    Some((handle, handle.read(cx).ok()?.chat()?.clone()))
                })
                .unwrap()
        });
        let (bridge, commands) = harness::spawn_recording();
        // Real hot reload replaces the root and all chat entities. Keep the old
        // source alive deliberately, proving the stable ID wins over its weak ref.
        let replacement = vcx.update(|window, cx| {
            let root = window.replace_root(cx, |_, cx| {
                let mut w = Workspace::for_test(learning::Coach::new(), cx);
                w.single_panel = true;
                w.set_test_bridge(bridge);
                w.push_test_panel("reload-source", cx);
                w
            });
            root.update(cx, |w, cx| w.focus_active(window, cx));
            root
        });
        vcx.run_until_parked();
        let new_source = replacement.read_with(vcx, |w, _| w.slots[0].panel.clone());
        assert_ne!(new_source, old_source);
        accounts.update(vcx, |_, cx| cx.emit(crate::panel::AccountsPanelClosed));
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            assert!(!cx.windows().contains(&accounts_window.into()));
            assert_eq!(cx.windows().len(), 1);
            assert!(
                new_source
                    .read(cx)
                    .input_focus_handle(cx)
                    .is_focused(window)
            );
            assert!(
                !old_source
                    .read(cx)
                    .input_focus_handle(cx)
                    .is_focused(window)
            );
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::RefreshRuntime { session_id }) if session_id == "reload-source")
        );
        assert!(commands.try_recv().is_err());
        replacement.read_with(vcx, |w, _| {
            assert_eq!(w.slots.len(), 1);
            assert_eq!(w.slots[0].panel, new_source);
        });
    }
}
