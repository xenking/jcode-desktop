//! One floating surface above the workspace at a time, after Zed's
//! `workspace::ModalLayer`.
//!
//! A modal is its own view. It owns its state and its keys, and closes itself
//! by emitting [`gpui::DismissEvent`]. The layer keeps at most one of them,
//! renders it above the canvas, and gives focus back to whatever held it before
//! the modal opened. A workspace therefore never carries a flag per modal or
//! a branch per modal in its key handlers: it asks the layer whether anything
//! is open.

use std::any::TypeId;

use gpui::{
    AnyView, AppContext as _, Context, DismissEvent, Empty, Entity, EventEmitter, FocusHandle,
    Focusable as _, IntoElement, ManagedView, Render, Subscription, Window,
};

/// A view the [`ModalLayer`] can show.
pub(crate) trait ModalView: ManagedView {
    /// Whether opening the modal moves keyboard focus into it. A notification
    /// that must not interrupt typing says no.
    fn takes_focus(&self) -> bool {
        true
    }
}

/// A modal restored without a window closed, so the layer could not hand
/// focus back. The owner decides where it goes.
pub(crate) struct HiddenWithoutFocus;

struct ActiveModal {
    view: AnyView,
    type_id: TypeId,
    focus: FocusHandle,
    /// Where focus returns on dismissal. `None` for a modal that never took it.
    previous_focus: Option<FocusHandle>,
    _dismiss: Subscription,
}

#[derive(Default)]
pub(crate) struct ModalLayer {
    active: Option<ActiveModal>,
}

impl EventEmitter<HiddenWithoutFocus> for ModalLayer {}

impl ModalLayer {
    pub(crate) fn has_active(&self) -> bool {
        self.active.is_some()
    }

    /// The open modal, when it is a `V`.
    pub(crate) fn active<V: ModalView>(&self) -> Option<Entity<V>> {
        let active = self.active.as_ref()?;
        (active.type_id == TypeId::of::<V>())
            .then(|| active.view.clone().downcast::<V>().ok())
            .flatten()
    }

    /// Open `build()`'s modal, or close it when a `V` is already open.
    pub(crate) fn toggle<V: ModalView>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) {
        if self.active::<V>().is_some() {
            self.hide(window, cx);
        } else {
            let modal = cx.new(|cx| build(window, cx));
            self.show(modal, window, cx);
        }
    }

    /// Show `modal`, replacing any other. Focus returns to where it was before
    /// the first modal of a replacement chain opened.
    pub(crate) fn show<V: ModalView>(
        &mut self,
        modal: Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = modal.focus_handle(cx);
        let replaced = self.active.take().and_then(|old| old.previous_focus);
        let previous_focus = modal.read(cx).takes_focus().then(|| {
            let previous = replaced.or_else(|| window.focused(cx));
            window.focus(&focus, cx);
            previous
        });
        let dismiss = cx.subscribe_in(&modal, window, |this, _, _: &DismissEvent, window, cx| {
            this.hide(window, cx)
        });
        self.install(modal, focus, previous_focus.flatten(), dismiss, cx);
    }

    /// Show a modal that was open before a reload. There is no window to read
    /// focus from, so dismissing it emits [`HiddenWithoutFocus`].
    pub(crate) fn restore<V: ModalView>(&mut self, modal: Entity<V>, cx: &mut Context<Self>) {
        let focus = modal.focus_handle(cx);
        let dismiss = cx.subscribe(&modal, |this, _, _: &DismissEvent, cx| {
            this.active = None;
            cx.emit(HiddenWithoutFocus);
            cx.notify();
        });
        self.install(modal, focus, None, dismiss, cx);
    }

    fn install<V: ModalView>(
        &mut self,
        modal: Entity<V>,
        focus: FocusHandle,
        previous_focus: Option<FocusHandle>,
        dismiss: Subscription,
        cx: &mut Context<Self>,
    ) {
        self.active = Some(ActiveModal {
            view: modal.into(),
            type_id: TypeId::of::<V>(),
            focus,
            previous_focus,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    pub(crate) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active.take() else {
            return;
        };
        // Someone who clicked elsewhere already moved focus. Leave it there.
        if let Some(previous) = active.previous_focus
            && active.focus.contains_focused(window, cx)
        {
            window.focus(&previous, cx);
        }
        cx.notify();
    }
}

impl Render for ModalLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match &self.active {
            Some(active) => active.view.clone().into_any_element(),
            None => Empty.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        InteractiveElement, ParentElement, Styled, TestAppContext,
        div, px,
    };

    struct Probe {
        focus: FocusHandle,
        takes_focus: bool,
    }

    impl Probe {
        fn new(takes_focus: bool, cx: &mut Context<Self>) -> Self {
            Self {
                focus: cx.focus_handle(),
                takes_focus,
            }
        }
    }

    impl gpui::Focusable for Probe {
        fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
            self.focus.clone()
        }
    }
    impl EventEmitter<DismissEvent> for Probe {}
    impl ModalView for Probe {
        fn takes_focus(&self) -> bool {
            self.takes_focus
        }
    }
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("probe")
                .track_focus(&self.focus)
                .size(px(40.0))
                .child("probe")
        }
    }

    struct Host {
        layer: Entity<ModalLayer>,
        editor: FocusHandle,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(div().id("editor").track_focus(&self.editor).size(px(40.0)))
                .child(self.layer.clone())
        }
    }

    fn host(cx: &mut TestAppContext) -> (Entity<Host>, &mut gpui::VisualTestContext) {
        let (host, vcx) = cx.add_window_view(|window, cx| {
            let editor = cx.focus_handle();
            window.focus(&editor, cx);
            Host {
                layer: cx.new(|_| ModalLayer::default()),
                editor,
            }
        });
        vcx.run_until_parked();
        (host, vcx)
    }

    fn show(host: &Entity<Host>, vcx: &mut gpui::VisualTestContext, takes_focus: bool) -> Entity<Probe> {
        let layer = host.read_with(vcx, |host, _| host.layer.clone());
        let probe = vcx.new(|cx| Probe::new(takes_focus, cx));
        vcx.update(|window, cx| {
            layer.update(cx, |layer, cx| layer.show(probe.clone(), window, cx))
        });
        vcx.run_until_parked();
        probe
    }

    #[gpui::test]
    fn dismissal_returns_focus_to_where_it_was(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let editor = host.read_with(vcx, |host, _| host.editor.clone());
        let probe = show(&host, vcx, true);
        vcx.update(|window, cx| {
            assert!(probe.focus_handle(cx).is_focused(window));
            assert!(!editor.is_focused(window));
        });
        probe.update(vcx, |_, cx| cx.emit(DismissEvent));
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            assert!(editor.is_focused(window));
            let layer = host.read(cx).layer.read(cx);
            assert!(!layer.has_active());
        });
    }

    #[gpui::test]
    fn a_modal_that_does_not_take_focus_leaves_it_alone(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let editor = host.read_with(vcx, |host, _| host.editor.clone());
        let probe = show(&host, vcx, false);
        vcx.update(|window, _| assert!(editor.is_focused(window)));
        probe.update(vcx, |_, cx| cx.emit(DismissEvent));
        vcx.run_until_parked();
        vcx.update(|window, _| assert!(editor.is_focused(window)));
    }

    #[gpui::test]
    fn dismissal_does_not_steal_focus_someone_moved_away(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let probe = show(&host, vcx, true);
        let elsewhere = vcx.update(|window, cx| {
            let handle = cx.focus_handle();
            window.focus(&handle, cx);
            handle
        });
        probe.update(vcx, |_, cx| cx.emit(DismissEvent));
        vcx.run_until_parked();
        vcx.update(|window, _| assert!(elsewhere.is_focused(window)));
    }

    #[gpui::test]
    fn replacing_a_modal_still_returns_to_the_original_focus(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let editor = host.read_with(vcx, |host, _| host.editor.clone());
        let _first = show(&host, vcx, true);
        let second = show(&host, vcx, true);
        let layer = host.read_with(vcx, |host, _| host.layer.clone());
        layer.read_with(vcx, |layer, _| assert!(layer.active::<Probe>() == Some(second.clone())));
        second.update(vcx, |_, cx| cx.emit(DismissEvent));
        vcx.run_until_parked();
        vcx.update(|window, _| assert!(editor.is_focused(window)));
    }

    #[gpui::test]
    fn toggle_opens_then_closes_the_same_kind(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let editor = host.read_with(vcx, |host, _| host.editor.clone());
        let layer = host.read_with(vcx, |host, _| host.layer.clone());
        for expect_open in [true, false] {
            vcx.update(|window, cx| {
                layer.update(cx, |layer, cx| {
                    layer.toggle(window, cx, |_, cx| Probe::new(true, cx))
                })
            });
            vcx.run_until_parked();
            layer.read_with(vcx, |layer, _| assert_eq!(layer.has_active(), expect_open));
        }
        vcx.update(|window, _| assert!(editor.is_focused(window)));
    }

    #[gpui::test]
    fn a_restored_modal_reports_dismissal_for_the_owner_to_place_focus(cx: &mut TestAppContext) {
        let (host, vcx) = host(cx);
        let layer = host.read_with(vcx, |host, _| host.layer.clone());
        let probe = vcx.new(|cx| Probe::new(true, cx));
        let hidden = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = hidden.clone();
        let _watch = vcx.update(|_, cx| {
            cx.subscribe(&layer, move |_, _: &HiddenWithoutFocus, _| seen.set(true))
        });
        layer.update(vcx, |layer, cx| layer.restore(probe.clone(), cx));
        vcx.run_until_parked();
        layer.read_with(vcx, |layer, _| assert!(layer.has_active()));
        probe.update(vcx, |_, cx| cx.emit(DismissEvent));
        vcx.run_until_parked();
        assert!(hidden.get());
        layer.read_with(vcx, |layer, _| assert!(!layer.has_active()));
    }
}
