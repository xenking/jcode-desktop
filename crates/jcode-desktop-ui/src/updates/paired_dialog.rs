use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, FocusHandle, Global, MouseButton, Window, WindowId, div, px, relative, rgba,
};
use jcode_desktop_updater::paired::BlockedAction;

struct Dialog {
    window_id: WindowId,
    sessions: Vec<String>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    action: async_channel::Sender<BlockedAction>,
}

struct BlockedDialog(Option<Dialog>);
impl Global for BlockedDialog {}

pub(super) fn show(
    sessions: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) -> (async_channel::Receiver<BlockedAction>, gpui::Subscription) {
    let window_id = window.window_handle().window_id();
    let previous_focus = window.focused(cx);
    let focus = cx.focus_handle();
    let (action, rx) = async_channel::bounded(1);
    cx.set_global(BlockedDialog(Some(Dialog {
        window_id,
        sessions,
        focus: focus.clone(),
        previous_focus,
        action,
    })));
    window.focus(&focus, cx);
    cx.refresh_windows();
    let closing = cx.on_window_closed(move |cx, closed| {
        if closed == window_id {
            dismiss(window_id, BlockedAction::Cancel, cx);
        }
    });
    (rx, closing)
}

fn dismiss(window_id: WindowId, action: BlockedAction, cx: &mut App) -> Option<FocusHandle> {
    let dialog = cx.try_global::<BlockedDialog>()?.0.as_ref()?;
    if dialog.window_id != window_id {
        return None;
    }
    let dialog = cx.global_mut::<BlockedDialog>().0.take()?;
    let _ = dialog.action.try_send(action);
    cx.refresh_windows();
    dialog.previous_focus
}

fn choose(action: BlockedAction, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if let Some(previous) = dismiss(id, action, cx) {
        window.focus(&previous, cx);
    }
    cx.stop_propagation();
}

pub(crate) fn render(window: &Window, cx: &App) -> Option<AnyElement> {
    let dialog = cx.try_global::<BlockedDialog>()?.0.as_ref()?;
    if dialog.window_id != window.window_handle().window_id() {
        return None;
    }
    let theme = Theme::global();
    let rows = div()
        .id("paired-update-blockers")
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(180.0))
        .overflow_y_scroll()
        .children(dialog.sessions.iter().map(|session| {
            div()
                .rounded_full()
                .px_3()
                .py_1()
                .bg(theme.HEADER_BG)
                .text_color(theme.TEXT)
                .overflow_hidden()
                .text_ellipsis()
                .child(session.clone())
        }));
    let actions = div().flex().flex_wrap().justify_end().gap_2().children(
        BlockedAction::ALL.into_iter().map(|action| {
            let primary = action == BlockedAction::RetryWhenIdle;
            let label = action.label();
            div()
                .id(label)
                .debug_selector(move || label.into())
                .px_4()
                .py_1()
                .rounded_full()
                .cursor_pointer()
                .bg(if primary {
                    theme.ACCENT
                } else {
                    theme.HEADER_BG
                })
                .text_color(if primary { theme.BG } else { theme.TEXT })
                .hover(|button| button.opacity(0.85))
                .on_click(move |_, window, cx| choose(action, window, cx))
                .child(label)
        }),
    );
    Some(div()
        .id("paired-update-overlay")
        .debug_selector(|| "paired-update-overlay".into())
        .track_focus(&dialog.focus)
        .on_key_down(|event, window, cx| {
            if event.keystroke.key == "escape" {
                choose(BlockedAction::Cancel, window, cx);
            }
        })
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(0x00000099))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .child(div()
            .w(px(480.0))
            .max_w(relative(0.92))
            .p_5()
            .rounded_xl()
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .bg(theme.PANEL_BG)
            .shadow_md()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(13.0))
            .text_color(theme.TEXT_DIM)
            .child(div().text_size(px(16.0)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.TEXT).child("Jcode update blocked"))
            .child("Jcode stopped waiting for these sessions.")
            .child(rows)
            .child(div().text_color(theme.WARN).child("Install now interrupts active turns. Retry when idle waits for active work to finish."))
            .child(actions))
        .into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn drain_progress_is_visible_in_the_main_workspace(cx: &mut gpui::TestAppContext) {
        use crate::updates::{self, UpdateState};

        let _update_guard = updates::test_lock();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx)
        });
        for retry in [false, true] {
            updates::set(UpdateState::PairDrain {
                blockers: vec!["turkey has an active turn".into()],
                retry,
            });
            workspace.update(vcx, |_, cx| cx.notify());
            vcx.run_until_parked();
            let chip = vcx
                .debug_bounds("update-chip")
                .expect("paired update status");
            let canvas = vcx
                .debug_bounds("workspace-canvas")
                .expect("workspace canvas");
            assert!(
                chip.top() >= canvas.bottom(),
                "status must not cover the composer"
            );
            assert_eq!(
                vcx.debug_bounds("cancel-paired-update-retry").is_some(),
                retry
            );
        }
        updates::set(UpdateState::Idle);
    }

    #[gpui::test]
    fn drain_dialog_dispatches_each_visible_action(cx: &mut gpui::TestAppContext) {
        let (_workspace, vcx) = cx.add_window_view(|_, cx| {
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx)
        });
        for expected in BlockedAction::ALL {
            let (actions, _closing) =
                vcx.update(|window, cx| show(vec!["turkey has an active turn".into()], window, cx));
            vcx.run_until_parked();
            for action in BlockedAction::ALL {
                assert!(
                    vcx.debug_bounds(action.label()).is_some(),
                    "{}",
                    action.label()
                );
            }
            let bounds = vcx.debug_bounds(expected.label()).unwrap();
            vcx.simulate_click(bounds.center(), gpui::Modifiers::none());
            vcx.run_until_parked();
            assert_eq!(actions.try_recv(), Ok(expected));
            assert!(vcx.debug_bounds("paired-update-overlay").is_none());
        }
    }
}
