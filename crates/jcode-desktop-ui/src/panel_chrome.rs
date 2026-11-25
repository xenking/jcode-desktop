//! Widgets and sizes every panel row shares: the composer's pills and icon
//! size, the hover cards that explain them, and the small action pills of
//! transcript cards. A leaf of the panel: nothing here knows the panel, so
//! each feature that draws a pill can use it without reaching a sibling.
use super::*;

/// Pill height.
pub(super) const TAB_HEIGHT: f32 = 22.;
/// One size for the effort circle, worker cluster, wait mark and cache timer.
pub(crate) const BAR_ICON: f32 = 13.;

pub(super) const OVERLAY_GAP: f32 = 4.;

/// Hover for the effort pill: the pill shows the dot alone, so the level name
/// has to live here.
pub(super) struct EffortTooltip(pub(super) String);

impl Render for EffortTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("EffortTooltip");
        div()
            .debug_selector(|| "composer-effort-tooltip".into())
            .max_w(px(300.))
            .p_2()
            .rounded_md()
            .bg(Theme::global().HEADER_BG)
            .text_size(px(12.))
            .text_color(Theme::global().TEXT)
            .child(self.0.clone())
    }
}

pub(super) fn composer_pill(id: &'static str) -> gpui::Stateful<gpui::Div> {
    composer_button(id, false).bg(Theme::global().chip_background())
}

pub(super) fn composer_button(id: &'static str, compact_hover: bool) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(TAB_HEIGHT))
        .px_2p5()
        .flex()
        .items_center()
        .gap_1p5()
        .rounded_full()
        .text_size(px(10.5))
        .font_family(theme.FONT_MONO)
        .text_color(theme.composer_secondary())
        .whitespace_nowrap()
        .cursor_pointer()
        .when(compact_hover, |el| el.group(id))
        .hover(|el| {
            let el = el.text_color(theme.TEXT);
            if compact_hover {
                el
            } else {
                el.bg(theme.chip_active_background())
            }
        })
}

/// `claude-opus-4-8` reads `Opus 4.8`. The provider already appears in the
/// method pill, so the redundant `Claude` family prefix is dropped, matching
/// the TUI's compact model label.
pub(super) fn pretty_model_label(model: Option<&str>) -> String {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return "Choose model".into();
    };
    let pretty = jcode_provider_core::model_names::pretty_model_display_name(model);
    match pretty.strip_prefix("Claude ") {
        Some(rest) if !rest.trim().is_empty() => rest.to_string(),
        _ => pretty,
    }
}

pub(super) struct CardActionTooltip(pub(super) &'static str);

impl gpui::Render for CardActionTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(theme.TOOL_BG)
            .text_size(px(11.))
            .text_color(theme.TEXT)
            .child(self.0)
    }
}


pub(super) fn embedded_reply_frame(
    input: &Entity<PromptInput>,
    selected: bool,
    window: &Window,
    cx: &App,
) -> gpui::Div {
    let theme = Theme::global();
    let focused = input.read(cx).focus_handle.is_focused(window);
    div()
        .flex_1()
        .min_w_0()
        .rounded(px(18.))
        .bg(if selected {
            theme.ACCENT_DIM
        } else {
            theme.INPUT_BG
        })
        .border_1()
        .border_color(if selected {
            theme.ACCENT
        } else if focused {
            theme.PANEL_BORDER_FOCUS
        } else {
            theme.INPUT_BORDER
        })
        .child(input.clone())
}

/// A compact pill which copies the complete original text.
pub(super) fn copy_pill(id: &'static str, index: usize, text: String) -> gpui::Stateful<gpui::Div> {
    action_pill(id, index, "Copy").on_click(move |_, _, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        cx.stop_propagation();
    })
}

pub(super) fn action_pill(id: &'static str, index: usize, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id((id, index))
        .debug_selector(move || id.into())
        .flex_none()
        .px_2p5()
        .py(px(2.))
        .rounded_full()
        .bg(Theme::global().ACCENT_DIM)
        .text_size(px(11.))
        .text_color(Theme::global().TEXT)
        .cursor_pointer()
        .hover(|el| el.opacity(0.8))
        .child(label)
}


