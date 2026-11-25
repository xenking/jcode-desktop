use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, FontWeight, Role, SharedString,
    StyleRefinement, Window, div, px, svg,
};

use super::{Tooltip, focus_ring};
use crate::theme::Theme;

/// What a [`Pill`] means. Colors and metrics come from the theme, so a pill
/// never hard-codes either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillVariant {
    /// The one filled accent action of a surface.
    Primary,
    /// A subtle filled pill for any other action.
    Secondary,
    /// A destructive action.
    Danger,
    /// A small inline action: secondary colors, tighter metrics.
    Compact,
    /// A quiet pill with no fill until hovered or selected, for toolbars,
    /// filters and icon buttons.
    Ghost,
}

impl PillVariant {
    fn is_small(self) -> bool {
        matches!(self, Self::Compact | Self::Ghost)
    }
}

/// What assistive technology sees for a pill. [`Pill::semantics`] is the one
/// place the role, label and availability are decided, and `render` applies
/// exactly this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PillSemantics {
    pub role: Role,
    /// The accessible name: the explicit `aria_label`, else the visible label.
    pub label: SharedString,
    /// Whether a click, or Enter or Space on the focused pill, runs a handler.
    pub actionable: bool,
    /// Whether the pill takes keyboard focus.
    pub focusable: bool,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A pill-shaped button. Always rounded fully, per the visual design rules.
///
/// `Pill` implements [`Styled`], so a caller can adjust sizes and spacing, and
/// [`ParentElement`], so extra children follow the label. Click handlers use
/// `on_click`, which GPUI also registers as the accessible Click action and
/// fires for Enter and Space on a focused element.
#[derive(IntoElement)]
pub struct Pill {
    id: ElementId,
    variant: PillVariant,
    label: Option<SharedString>,
    aria_label: Option<SharedString>,
    icon: Option<&'static [u8]>,
    glyph: Option<SharedString>,
    selected: bool,
    disabled: bool,
    tooltip: Option<Tooltip>,
    selector: Option<SharedString>,
    focus: Option<FocusHandle>,
    tab_index: isize,
    on_click: Option<ClickHandler>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl Pill {
    pub fn new(
        variant: PillVariant,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            variant,
            label: Some(label.into()),
            aria_label: None,
            icon: None,
            glyph: None,
            selected: false,
            disabled: false,
            tooltip: None,
            selector: None,
            focus: None,
            tab_index: 0,
            on_click: None,
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }

    pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(PillVariant::Primary, id, label)
    }

    pub fn secondary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(PillVariant::Secondary, id, label)
    }

    pub fn danger(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(PillVariant::Danger, id, label)
    }

    pub fn compact(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(PillVariant::Compact, id, label)
    }

    pub fn ghost(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(PillVariant::Ghost, id, label)
    }

    /// A round, icon-only ghost button. It has no visible text, so the
    /// accessible name is required.
    pub fn icon_button(
        id: impl Into<ElementId>,
        icon: &'static [u8],
        aria_label: impl Into<SharedString>,
    ) -> Self {
        let mut pill = Self::new(PillVariant::Ghost, id, "");
        pill.label = None;
        pill.aria_label = Some(aria_label.into());
        pill.icon = Some(icon);
        pill.size(px(28.)).p_0()
    }

    /// A leading SVG glyph, drawn in the pill's text color.
    pub fn icon(mut self, icon: &'static [u8]) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A leading text glyph such as `✓` or `→`, for icon vocabularies that
    /// ship characters instead of SVG.
    pub fn glyph(mut self, glyph: impl Into<SharedString>) -> Self {
        self.glyph = Some(glyph.into());
        self
    }

    /// Marks the current choice of a filter or toggle row.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// A disabled pill is dimmed, skips the tab order and runs no handler.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Overrides the accessible name, which defaults to the visible label.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// A hover [`Tooltip`]. Its key hint is also exposed as the pill's
    /// accessible keyboard shortcut.
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    pub fn debug_selector(mut self, selector: impl Into<SharedString>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    /// Uses a caller-owned focus handle, so the caller can focus the pill or
    /// observe its focus.
    pub fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.focus = Some(handle.clone());
        self
    }

    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// The role, accessible name and availability this pill renders with.
    pub fn semantics(&self) -> PillSemantics {
        let label = self
            .aria_label
            .clone()
            .or_else(|| self.label.clone())
            .unwrap_or_default();
        debug_assert!(
            !label.is_empty(),
            "an icon-only Pill needs an accessible name"
        );
        PillSemantics {
            role: Role::Button,
            label,
            actionable: !self.disabled && self.on_click.is_some(),
            focusable: !self.disabled,
        }
    }
}

impl Styled for Pill {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Pill {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Pill {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = Theme::global();
        let semantics = self.semantics();
        let variant = self.variant;
        let small = variant.is_small();
        let selected = self.selected;
        let (bg, fg) = match (variant, selected) {
            (PillVariant::Primary, _) => (Some(theme.ACCENT), theme.BG),
            (PillVariant::Secondary | PillVariant::Compact, false) => {
                (Some(theme.INLINE_CODE_BG), theme.TEXT)
            }
            (PillVariant::Secondary | PillVariant::Compact, true) => {
                (Some(theme.ACCENT_DIM), theme.TEXT)
            }
            (PillVariant::Danger, _) => (Some(theme.ERROR_BG), theme.ERROR),
            (PillVariant::Ghost, false) => (None, theme.TEXT_DIM),
            (PillVariant::Ghost, true) => (Some(theme.TOOL_BG), theme.TEXT),
        };

        let mut el = div()
            .id(self.id)
            .role(semantics.role)
            .aria_label(semantics.label.clone())
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1()
            .px(px(if small { 8. } else { 14. }))
            .py(px(if small { 2. } else { 5. }))
            .rounded_full()
            .text_size(px(if small { 11. } else { 13. }))
            .when(matches!(variant, PillVariant::Primary), |el| {
                el.font_weight(FontWeight::SEMIBOLD)
            })
            .when_some(bg, |el, bg| el.bg(bg))
            .text_color(fg);
        if let Some(selector) = self.selector {
            el = el.debug_selector(move || selector.to_string());
        }
        // Caller adjustments win over the variant's metrics.
        el.style().refine(&self.style);

        if semantics.focusable {
            el = match &self.focus {
                Some(handle) => {
                    el.track_focus(&handle.clone().tab_index(self.tab_index).tab_stop(true))
                }
                None => el.tab_index(self.tab_index),
            }
            .focus_visible(focus_ring)
            .cursor_pointer()
            .hover(move |el| {
                if matches!(variant, PillVariant::Ghost) {
                    el.bg(theme.TOOL_BG).text_color(theme.TEXT)
                } else {
                    el.opacity(0.85)
                }
            });
        } else {
            el = el.opacity(0.45);
        }
        if let Some(tooltip) = self.tooltip {
            if let Some(hint) = tooltip.key_hint_text() {
                el = el.aria_keyshortcuts(hint);
            }
            el = el.tooltip(tooltip.builder());
        }
        if semantics.actionable
            && let Some(handler) = self.on_click
        {
            el = el.on_click(move |event, window, cx| handler(event, window, cx));
        }
        el.when_some(self.icon, |el, icon| {
            el.child(
                svg()
                    .data(icon)
                    .size(px(if small { 12. } else { 14. }))
                    .flex_none()
                    .text_color(fg),
            )
        })
        .children(self.glyph)
        .children(self.label)
        .children(self.children)
    }
}
