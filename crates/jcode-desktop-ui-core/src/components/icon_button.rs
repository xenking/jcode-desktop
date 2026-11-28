use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, ElementId, SharedString, StyleRefinement, Window, px};

use super::{Pill, PillVariant, Tooltip};

/// A round, icon-only control for a toolbar or header. It floats above the row
/// it sits on, so a press neither reaches whatever is behind it nor takes
/// keyboard focus from the composer, and its debug selector is its id.
///
/// The accessible name is required because there is no visible text. Give it a
/// built-in SVG with [`IconButton::icon`], or any element as a child.
#[derive(IntoElement)]
pub struct IconButton {
    pill: Pill,
}

impl IconButton {
    pub fn new(id: impl Into<SharedString>, aria_label: impl Into<SharedString>) -> Self {
        let id = id.into();
        let pill = Pill::unlabeled(PillVariant::Ghost, ElementId::from(id.clone()), aria_label)
            .debug_selector(id)
            .size(px(28.))
            .p_0()
            .occlude()
            .keep_focus();
        Self { pill }
    }

    /// A leading SVG icon, `size` pixels square, drawn in the button's text color.
    pub fn icon(mut self, icon: &'static [u8], size: f32) -> Self {
        self.pill = self.pill.icon(icon).icon_size(size);
        self
    }

    /// A hover [`Tooltip`], whose key hint is also the accessible shortcut.
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.pill = self.pill.tooltip(tooltip);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.pill = self.pill.on_click(handler);
        self
    }
}

impl Styled for IconButton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.pill.style()
    }
}

impl ParentElement for IconButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.pill.extend(elements);
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.pill.render(window, cx)
    }
}
