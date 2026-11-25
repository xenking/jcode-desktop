//! The always-visible entry points at the top of the thread list. It sits above
//! the worktree and collection views because it starts either one.
use super::sidebar_atoms::{BRANCH_ICON, COLLECTION_ICON, FOLDER_ICON, HIDE_SIDEBAR_ICON, icon};
use super::*;
use crate::components::Tooltip;

impl Workspace {
    /// Always-visible entry points at the top of the thread list.
    pub(super) fn render_sidebar_quick_actions(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let pill = |id: &'static str, label: &'static str, glyph: gpui::AnyElement| {
            let tip = match id {
                "sidebar-open-project" => "Open a project folder",
                _ => "New thread in the default directory",
            };
            div()
                .id(id)
                .debug_selector(move || id.into())
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .h(px(26.0))
                .px_2()
                .flex()
                .items_center()
                .justify_center()
                .gap_1()
                .rounded_full()
                .cursor_pointer()
                .bg(Theme::global().TOOL_BG)
                .text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .hover(|el| {
                    el.bg(Theme::global().PANEL_BG)
                        .text_color(Theme::global().TEXT)
                })
                .tooltip(Tooltip::text(tip).builder())
                .child(glyph)
                .child(div().min_w_0().truncate().child(label))
        };
        div()
            .flex_none()
            .w_full()
            .min_w_0()
            .pl_2()
            .pr(px(crate::scrollbar::GUTTER + 4.0))
            .pt_2()
            .flex()
            .gap_2()
            .child(
                pill(
                    "sidebar-new-thread",
                    "Thread",
                    div().child("+").into_any_element(),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.missed("new_panel", cx);
                        this.open_new_session(cx);
                    }),
                ),
            )
            .child(
                pill("sidebar-open-project", "Project", icon(FOLDER_ICON, 12.0)).on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.open_folder(&OpenFolder, window, cx);
                    }),
                ),
            )
            .child(
                // Icon-only, like hide: four labelled pills do not fit 224px.
                div()
                    .id("sidebar-new-collection")
                    .debug_selector(|| "sidebar-new-collection".into())
                    .flex_none()
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(Theme::global().TOOL_BG)
                    .hover(|el| el.bg(Theme::global().PANEL_BG))
                    .tooltip(Tooltip::text("New collection: group chats your own way").builder())
                    .child(icon(COLLECTION_ICON, 13.0))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.begin_new_collection(None, window, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("sidebar-hide")
                    .debug_selector(|| "sidebar-hide".into())
                    .flex_none()
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(Theme::global().TOOL_BG)
                    .hover(|el| el.bg(Theme::global().PANEL_BG))
                    .tooltip(Tooltip::text("Hide sidebar").key_hint("Ctrl+B").builder())
                    .child(icon(HIDE_SIDEBAR_ICON, 14.0))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            if this.compact_sidebar_open {
                                // Narrow windows show the sidebar as a drawer:
                                // just dismiss it and keep the menu button.
                                this.compact_sidebar_open = false;
                                this.focus_pending = true;
                                cx.notify();
                            } else {
                                this.toggle_sidebar(&ToggleSidebar, window, cx);
                            }
                        }),
                    ),
            )
            .into_any_element()
    }
}
