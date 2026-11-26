//! A leaf view's window onto the panel that owns it.
//!
//! Hover cards, pills and outlines draw facts the panel keeps, and must redraw
//! when it changes. Holding the panel itself would make every such view know
//! the whole panel, and the panel knows them. A `Live` is made by the panel
//! with the one question the view asks, so the view knows only the answer.
use std::rc::Rc;

use gpui::{App, Context, Entity};

type OnChange = Box<dyn FnMut(&mut App)>;

/// What a view reads from its owner, and how to be told when it changes.
pub(crate) struct Live<T> {
    read: Rc<dyn Fn(&App) -> Option<T>>,
    observe: Rc<dyn Fn(OnChange, &mut App)>,
}

impl<T> Clone for Live<T> {
    fn clone(&self) -> Self {
        Self {
            read: self.read.clone(),
            observe: self.observe.clone(),
        }
    }
}

impl<T: 'static> Live<T> {
    /// Reads `read` of `owner` for as long as it lives.
    pub(crate) fn of<O: 'static>(
        owner: &Entity<O>,
        read: impl Fn(&O, &App) -> T + 'static,
    ) -> Self {
        let weak = owner.downgrade();
        let watched = weak.clone();
        Self {
            read: Rc::new(move |cx| {
                let owner = weak.upgrade()?;
                Some(read(owner.read(cx), cx))
            }),
            observe: Rc::new(move |mut on_change, cx| {
                let Some(owner) = watched.upgrade() else {
                    return;
                };
                cx.observe(&owner, move |_, cx| on_change(cx)).detach();
            }),
        }
    }

    /// A source whose owner is already gone.
    #[cfg(test)]
    pub(crate) fn gone() -> Self {
        Self {
            read: Rc::new(|_| None),
            observe: Rc::new(|_, _| {}),
        }
    }

    /// The current answer, or `None` once the owner is gone.
    pub(crate) fn get(&self, cx: &App) -> Option<T> {
        (self.read)(cx)
    }

    /// Redraw `viewer` whenever the owner changes.
    pub(crate) fn follow<V: 'static>(&self, cx: &mut Context<V>) {
        let viewer = cx.entity_id();
        (self.observe)(Box::new(move |cx| cx.notify(viewer)), cx);
    }

    /// Hands every new answer to `viewer`, without touching it while
    /// `differs` says it already has it: a view that is leased on every
    /// owner change would dirty whatever retained layout holds it.
    pub(crate) fn watch<V: 'static>(
        &self,
        cx: &mut Context<V>,
        differs: impl Fn(&V, &T) -> bool + 'static,
        apply: impl Fn(&mut V, T, &mut Context<V>) + 'static,
    ) {
        let viewer = cx.weak_entity();
        let read = self.read.clone();
        (self.observe)(
            Box::new(move |cx| {
                let Some(value) = read(cx) else {
                    return;
                };
                let stale = viewer
                    .read_with(cx, |viewer, _| differs(viewer, &value))
                    .unwrap_or(false);
                if stale {
                    let _ = viewer.update(cx, |viewer, cx| apply(viewer, value, cx));
                }
            }),
            cx,
        );
    }
}
