//! `menu` actions for the chat panel's cards and popups.
//!
//! The panel root joins the `menu` key context only while one of these
//! surfaces is open, so Escape, Enter and the arrows keep their composer
//! meanings the rest of the time. A handler that has nothing to act on calls
//! `cx.propagate()` so the key still reaches the focused child.
//!
//! Left to raw key listeners on purpose: the ask card (letter shortcuts and an
//! inline editor), the routing gate's Tab and digit answers, the rewind
//! selection's Left and Right, and the queue card's Alt+arrows and `M`.

use super::*;
use crate::menu;
use jcode_session_types::routing_gate::GateKey;

impl Panel {
    pub(super) fn key_context(&self) -> gpui::KeyContext {
        let mut context = gpui::KeyContext::default();
        context.add("ChatPanel");
        if self.menu_open() {
            context.add(menu::CONTEXT);
        }
        context
    }

    fn menu_open(&self) -> bool {
        self.context_usage.is_open()
            || self.swarm_list_open
            || self.routing_popover.is_some()
            || self.login.is_some()
            || self.recovery_picker_open
            || self.diff_review.is_some()
            || self.image_preview.is_some()
            || self.rewind_selection.is_some()
            || self.prompt_queue.card_menu.is_some()
            || self.routing_gate.is_some()
    }

    pub(super) fn menu_cancel(
        &mut self,
        _: &menu::Cancel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_usage.is_open() {
            self.close_context_usage(cx);
            self.focus_input(window, cx);
        } else if self.finish_rewind(false, window, cx)
            || self.cancel_queue_card(window, cx)
            || self.press_routing_gate(GateKey::Esc, window, cx)
        {
        } else if self.swarm_list_open {
            self.close_swarm_to_trigger(window, cx);
        } else if self.routing_popover.is_some() {
            self.close_routing_popover(window, cx);
        } else if self.login.is_some() {
            self.close_login_picker(cx);
            self.focus_input(window, cx);
        } else if self.recovery_picker_open {
            self.recovery_picker_open = false;
            self.focus_input(window, cx);
            cx.notify();
        } else if self.diff_review.is_some() {
            self.close_diff_review(window, cx);
        } else if self.image_preview.is_some() {
            self.close_image_preview(window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn menu_confirm(
        &mut self,
        _: &menu::Confirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !(self.confirm_rewind_selection(window, cx)
            || self.confirm_queue_card(window, cx)
            || self.press_routing_gate(GateKey::Enter, window, cx))
        {
            cx.propagate();
        }
    }

    pub(super) fn menu_select_next(
        &mut self,
        _: &menu::SelectNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !(self.step_rewind_message(false, cx)
            || self.press_routing_gate(GateKey::Down, window, cx))
        {
            cx.propagate();
        }
    }

    pub(super) fn menu_select_previous(
        &mut self,
        _: &menu::SelectPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !(self.step_rewind_message(true, cx)
            || self.press_routing_gate(GateKey::Up, window, cx))
        {
            cx.propagate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
        cx.update(crate::bind_workspace_keys);
        cx.add_window_view(|_, cx| {
            Panel::new(
                "panel-menu".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        })
    }

    #[gpui::test]
    fn menu_context_follows_open_cards(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = panel(cx);
        let has_menu = |vcx: &mut gpui::VisualTestContext| {
            panel.read_with(vcx, |panel, _| panel.key_context().contains(menu::CONTEXT))
        };
        assert!(!has_menu(vcx), "a quiet composer keeps its own Escape");
        panel.update(vcx, |panel, cx| panel.open_login_picker(cx));
        assert!(has_menu(vcx));
        panel.update(vcx, |panel, cx| panel.close_login_picker(cx));
        assert!(!has_menu(vcx));
    }

    #[gpui::test]
    fn escape_closes_the_login_picker_and_returns_to_the_composer(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = panel(cx);
        panel.update(vcx, |panel, cx| panel.open_login_picker(cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-dialog").is_some());
        // The search field closes the picker itself. Focus the panel to
        // exercise the root's `menu::Cancel`.
        let focus = panel.read_with(vcx, |panel, _| panel.focus_handle.clone());
        vcx.update(|window, cx| window.focus(&focus, cx));
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-dialog").is_none());
        assert!(panel.read_with(vcx, |panel, _| panel.login.is_none()));
        assert!(vcx.update(|window, cx| panel.read(cx).input_focus_handle(cx).is_focused(window)));
    }

    #[gpui::test]
    fn escape_closes_the_recovery_picker(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = panel(cx);
        panel.update(vcx, |panel, cx| {
            panel.recovery_picker_open = true;
            cx.notify();
        });
        vcx.run_until_parked();
        let focus = panel.read_with(vcx, |panel, _| panel.focus_handle.clone());
        vcx.update(|window, cx| window.focus(&focus, cx));
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(!panel.read_with(vcx, |panel, _| panel.recovery_picker_open));
    }
}
