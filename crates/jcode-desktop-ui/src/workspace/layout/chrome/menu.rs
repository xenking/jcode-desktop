//! Shared navigation actions for popups, cards and dialogs, after Zed's
//! `menu` namespace.
//!
//! An element that owns a dismissible surface adds [`CONTEXT`] to its key
//! context and listens for these actions with `.on_action(cx.listener(..))`.
//! The keys live in one place ([`bind_keys`]) instead of being matched as raw
//! key strings in every view, so they can be rebound and appear in key hints.
//!
//! Actions that nobody handles fall through to the raw key listeners and to
//! text input, so a context added to an element never swallows a key by
//! itself. A handler that does not own the key in the current state calls
//! `cx.propagate()`.

use gpui::{App, KeyBinding};

gpui::actions!(
    menu,
    [
        Confirm,
        Cancel,
        SelectNext,
        SelectPrevious,
        SelectFirst,
        SelectLast
    ]
);

/// Key context shared by every popup, card and dialog.
pub(crate) const CONTEXT: &str = "menu";

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement};

    struct Probe {
        focus: FocusHandle,
        seen: Vec<&'static str>,
        with_context: bool,
    }

    impl gpui::Render for Probe {
        fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
            let root = gpui::div()
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &Cancel, _, _| this.seen.push("cancel")))
                .on_action(cx.listener(|this, _: &Confirm, _, _| this.seen.push("confirm")))
                .on_action(cx.listener(|this, _: &SelectNext, _, _| this.seen.push("next")))
                .on_action(cx.listener(|this, _: &SelectPrevious, _, _| this.seen.push("prev")))
                .on_action(cx.listener(|this, _: &SelectFirst, _, _| this.seen.push("first")))
                .on_action(cx.listener(|this, _: &SelectLast, _, _| this.seen.push("last")));
            if self.with_context {
                root.key_context(CONTEXT).child("probe")
            } else {
                root.child("probe")
            }
        }
    }

    impl Focusable for Probe {
        fn focus_handle(&self, _: &App) -> FocusHandle {
            self.focus.clone()
        }
    }

    fn press(with_context: bool, keys: &str, cx: &mut gpui::TestAppContext) -> Vec<&'static str> {
        cx.update(|cx| {
            cx.clear_key_bindings();
            bind_keys(cx);
        });
        let (probe, vcx) = cx.add_window_view(|_, cx| Probe {
            focus: cx.focus_handle(),
            seen: Vec::new(),
            with_context,
        });
        vcx.update(|window, cx| {
            let focus = probe.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes(keys);
        probe.read_with(vcx, |probe, _| probe.seen.clone())
    }

    #[gpui::test]
    fn menu_context_maps_navigation_keys_to_actions(cx: &mut gpui::TestAppContext) {
        assert_eq!(
            press(true, "escape enter down up home end", cx),
            ["cancel", "confirm", "next", "prev", "first", "last"]
        );
    }

    #[gpui::test]
    fn keys_do_nothing_outside_a_menu_context(cx: &mut gpui::TestAppContext) {
        assert!(press(false, "escape enter down up home end", cx).is_empty());
    }
}
