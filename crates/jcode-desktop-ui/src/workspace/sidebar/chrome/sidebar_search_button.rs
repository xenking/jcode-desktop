//! The sidebar's magnifier: the pointer route into global search.
//!
//! Two shapes, one action. The wide sidebar has a pill row with words in it, so
//! search appears there as a pill. The compact sidebar only has room for round
//! icon buttons next to the hamburger, so it gets the magnifier alone. Both
//! dispatch the same [`GlobalSearch`] action the keybinding uses.

use super::*;
use crate::components::{IconButton, Tooltip};

/// The search glyph, shared by both button shapes.
fn search_icon(size: f32, color: impl Into<gpui::Hsla>) -> gpui::Svg {
    gpui::svg()
        .data(include_bytes!("../../../../../../assets/icons/tools/session_search.svg").as_slice())
        .size(px(size))
        .text_color(color)
}

fn search_tooltip() -> Tooltip {
    Tooltip::text("Search all sessions").key_hint("Cmd+Shift+F")
}

impl Workspace {
    /// The wide sidebar's pill, matching the buttons beside it.
    pub(super) fn render_sidebar_search_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        div()
            .id("sidebar-search")
            .debug_selector(|| "sidebar-search".into())
            .flex_none()
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .px_2()
            .py_1()
            .rounded_t_md()
            .h(px(30.0))
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_size(px(11.0))
            .text_color(theme.TEXT_DIM)
            .hover(|el| el.bg(theme.HEADER_BG).text_color(theme.TEXT))
            .tooltip(search_tooltip().builder())
            .role(gpui::Role::Button)
            .aria_label("Search all sessions")
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.prevent_default()
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_session_search(&GlobalSearch, window, cx)
            }))
            .child(search_icon(12.0, theme.TEXT_DIM))
            .child("search")
            .into_any_element()
    }

    /// The compact header's round icon button, sized like the hamburger next
    /// to it.
    pub(super) fn render_sidebar_search_icon(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        IconButton::new("sidebar-search-icon", "Search all sessions")
            .tooltip(search_tooltip())
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.open_session_search(&GlobalSearch, window, cx)
            }))
            .child(search_icon(13.0, theme.TEXT))
            .into_any_element()
    }
}
