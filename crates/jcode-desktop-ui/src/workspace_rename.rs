//! Manual titles use the native SDK operation, never a desktop-only override.
use super::*;
use gpui::DismissEvent;

/// The rename dialog. It captures its target panel, not the active index,
/// because navigation may change while editing.
type SendTitle = dyn Fn(&Entity<Panel>, String, &mut App) -> Option<Result<(), &'static str>>;

pub(super) struct Rename {
    send_title: Box<SendTitle>,
    target: Entity<Panel>,
    input: Entity<PromptInput>,
    error: Option<String>,
}

fn validated_title(title: &str) -> Result<String, &'static str> {
    let title = title.trim();
    if title.is_empty() {
        Err("Enter a session title.")
    } else if title.chars().any(char::is_control) {
        Err("Use a single line for the session title.")
    } else {
        Ok(title.to_owned())
    }
}

impl Rename {
    fn new(
        send_title: Box<SendTitle>,
        target: Entity<Panel>,
        title: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        let cancel = weak.clone();
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(cx, "Session title", move |title, _, _, app| {
                let _ = weak.update(app, |this, cx| this.save(&title, cx));
            })
            .without_command_completion()
            .with_on_overlay_cancel(move |app| {
                let _ = cancel.update(app, |_, cx| cx.emit(DismissEvent));
                true
            });
            input.set_content(title, cx);
            let mut snapshot = input.snapshot();
            snapshot.selection_start = 0;
            snapshot.selection_end = snapshot.content.len();
            input.restore(snapshot, cx);
            input
        });
        Self {
            send_title,
            target,
            input,
            error: None,
        }
    }

    fn save(&mut self, title: &str, cx: &mut Context<Self>) {
        let title = match validated_title(title) {
            Ok(title) => title,
            Err(error) => {
                self.error = Some(error.into());
                cx.notify();
                return;
            }
        };
        let target = self.target.clone();
        let sent = (self.send_title)(&target, title, cx);
        match sent {
            // SessionRenamed is authoritative and updates both tabs and history.
            // Transport failures use the existing visible session error path.
            Some(Ok(())) | None => cx.emit(DismissEvent),
            Some(Err(error)) => {
                self.error = Some(error.into());
                cx.notify();
            }
        }
    }

    fn save_current(&mut self, cx: &mut Context<Self>) {
        let title = self.input.read(cx).text_owned();
        self.save(&title, cx);
    }
}

impl gpui::EventEmitter<DismissEvent> for Rename {}
impl ModalView for Rename {}

impl Focusable for Rename {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).editor_focus()
    }
}

impl Workspace {
    pub(super) fn rename_target(&self, cx: &App) -> Option<Entity<Panel>> {
        let slot = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)?;
        let panel = slot.panel.chat()?;
        let state = panel.read(cx);
        state.can_rename().then(|| panel.clone())
    }

    pub(super) fn rename_session(
        &mut self,
        _: &RenameSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if let Some(open) = self.active_modal::<Rename>(cx) {
            let focus = open.focus_handle(cx);
            window.focus(&focus, cx);
            return;
        }
        let Some(target) = self.rename_target(cx) else {
            return;
        };
        let title = target.read(cx).folder_title();
        let workspace = cx.weak_entity();
        self.modal_layer.update(cx, |layer, cx| {
            layer.toggle(window, cx, |_, cx| Rename::new(Box::new(move |target, title, app| {
                workspace.update(app, |workspace, cx| workspace.send_rename(target, title, cx)).ok()
            }), target, title, cx))
        });
    }

    /// Send the rename for `target` unless it can no longer take one. The
    /// error is the dialog's to show.
    fn send_rename(
        &mut self,
        target: &Entity<Panel>,
        title: String,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        if !self
            .slots
            .iter()
            .any(|slot| slot.panel.chat() == Some(target) && !slot.closing)
        {
            return Err("This session has been closed.");
        }
        if target.read(cx).is_busy() {
            return Err("Wait for this session to finish working, then save your title.");
        }
        self.bridge.send(Command::SessionOperation {
            session_id: target.read(cx).session_id.clone(),
            operation: harness::SessionOperation::Rename(Some(title)),
        });
        Ok(())
    }
}

impl Render for Rename {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("rename-session-overlay")
            .debug_selector(|| "rename-session-overlay".into())
            .key_context(crate::menu::CONTEXT)
            .on_action(cx.listener(|_, _: &crate::menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.45))
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, _, cx| {
                    cx.emit(DismissEvent);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .id("rename-session-dialog")
                    .debug_selector(|| "rename-session-dialog".into())
                    .w(px(420.0))
                    .max_w_full()
                    .mx_4()
                    .p_4()
                    .rounded_lg()
                    .bg(Theme::global().PANEL_BG)
                    .border_1()
                    .border_color(Theme::global().PANEL_BORDER)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .capture_action(cx.listener(|this, _: &crate::input::Submit, _, cx| {
                        cx.stop_propagation();
                        this.save_current(cx);
                    }))
                    .child(div().text_size(px(16.0)).child("Rename session"))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .child("Enter to save · Esc to cancel · F2 to rename"),
                    )
                    .child(self.input.clone())
                    .when_some(self.error.clone(), |el, error| {
                        el.child(
                            div()
                                .text_size(px(12.0))
                                .text_color(Theme::global().ERROR)
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("rename-session-cancel")
                                    .debug_selector(|| "rename-session-cancel".into())
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
                                    .child("Cancel"),
                            )
                            .child(
                                div()
                                    .id("rename-session-save")
                                    .debug_selector(|| "rename-session-save".into())
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(Theme::global().TOOL_BG)
                                    .text_color(Theme::global().ACCENT)
                                    .on_click(cx.listener(|this, _, _, cx| this.save_current(cx)))
                                    .child("Save"),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "workspace_rename_tests.rs"]
mod tests;
