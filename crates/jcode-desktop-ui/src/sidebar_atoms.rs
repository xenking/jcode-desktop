//! Header atoms shared by the sidebar groups: icons and the pill buttons on
//! project, checkout and collection headers. The lowest layer of the sidebar
//! group views, which all draw with it and none of which it knows about.
use super::*;
use crate::components::Tooltip;

pub(super) const BRANCH_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="black" stroke-width="1.6"><circle cx="6" cy="5" r="2.5"/><circle cx="18" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><path d="M6 7.5v9M18 7.5c0 6-12 2-12 8"/></g></svg>"#;
pub(super) const FOLDER_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Zm0 3h18" fill="none" stroke="black" stroke-width="1.6" stroke-linejoin="round"/></svg>"#;
pub(super) const HIDE_SIDEBAR_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="black" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16M16 9.5 13.5 12l2.5 2.5"/></g></svg>"#;
pub(super) const COLLECTION_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="black" stroke-width="1.6" stroke-linejoin="round"><rect x="3" y="8" width="18" height="12" rx="2.5"/><path d="M6 5h12M8.5 2.5h7"/></g></svg>"#;

/// A compact pill button used by sidebar project and checkout headers. It acts
/// on mouse-down and stops propagation so the header never toggles as well.
pub(super) fn header_action(
    id: gpui::SharedString,
    tooltip: &'static str,
    content: gpui::AnyElement,
    visible: bool,
    group: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .flex_none()
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .text_size(px(13.0))
        .text_color(Theme::global().TEXT_DIM)
        .opacity(if visible { 1.0 } else { 0.0 })
        .group_hover(group, |el| el.opacity(1.0))
        .hover(|el| {
            el.bg(Theme::global().PANEL_BG)
                .text_color(Theme::global().TEXT)
        })
        .tooltip(Tooltip::text(tooltip).builder())
        .child(content)
}

/// Hover-only header actions. They float over the right end of the row, so the
/// label and branch pill use the full width until the pointer arrives. The
/// parent must be `relative()`; `right` leaves room for trailing content.
pub(super) fn hover_actions(group: &'static str, right: f32) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right(px(right))
        .flex()
        .items_center()
        .gap_1()
        .pl_1()
        .rounded_full()
        .bg(Theme::global().TOOL_BG)
        .opacity(0.0)
        .group_hover(group, |el| el.opacity(1.0))
}

pub(super) fn icon(data: &'static [u8], size: f32) -> gpui::AnyElement {
    gpui::svg()
        .data(data)
        .size(px(size))
        .flex_none()
        .text_color(Theme::global().TEXT_DIM)
        .into_any_element()
}
