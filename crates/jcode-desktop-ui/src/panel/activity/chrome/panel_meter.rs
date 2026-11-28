//! Meter cards and hover tooltips. The quota, context, cache and swarm rows
//! all draw the same rows-and-bar card, so the card lives below every one of
//! them and knows none of them.
use super::*;

/// Compact structured hover for multi-fact sidebar and composer controls.
pub(crate) struct RowsTooltip {
    pub(crate) title: String,
    pub(crate) rows: Vec<String>,
}

impl RowsTooltip {
    pub(crate) fn separated(title: impl Into<String>, text: &str) -> Self {
        Self {
            title: title.into(),
            rows: text
                .lines()
                .flat_map(|line| line.split(" · "))
                .filter(|row| !row.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    fn content(&self, width: gpui::Pixels) -> gpui::Div {
        let theme = Theme::global();
        div()
            .debug_selector(|| "rows-tooltip".into())
            .px_3()
            .py_2()
            .w(width)
            .rounded_xl()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(11.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .debug_selector(|| "rows-tooltip-title".into())
                    .whitespace_normal()
                    .text_color(theme.TEXT)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(self.title.clone()),
            )
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                div()
                    .debug_selector(move || format!("rows-tooltip-row-{index}"))
                    .flex_none()
                    .line_height(px(16.))
                    .whitespace_normal()
                    .text_color(theme.TEXT_DIM)
                    .child(row.clone())
            }))
    }
}

impl Render for RowsTooltip {
    /// A tooltip is laid out with no width to wrap in, so a long row would be
    /// measured as one line and then painted over the next row. A definite
    /// width lets every row take the height of its wrapped text.
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.content(px(320.0).min(window.viewport_size().width))
    }
}

pub(crate) struct MeterTooltip(pub(crate) String);
impl Render for MeterTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("MeterTooltip");
        let theme = Theme::global();
        div()
            .px_3()
            .py_2()
            .max_w(px(380.))
            .rounded_md()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(11.))
            .text_color(theme.TEXT_DIM)
            .child(self.0.clone())
    }
}

pub(crate) fn meter_card(theme: &Palette) -> gpui::Div {
    div()
        .px_3()
        .py_2()
        .min_w(px(220.))
        .rounded_xl()
        .bg(theme.HEADER_BG)
        .border_1()
        .border_color(theme.PANEL_BORDER)
        .text_size(px(11.))
        .flex()
        .flex_col()
        .gap_1p5()
}

pub(crate) struct MeterIds {
    pub row: String,
    pub value: String,
    pub bar: String,
}

pub(crate) fn meter_row(
    ids: MeterIds,
    name: String,
    value: String,
    bar: Option<(f32, gpui::Rgba)>,
    detail: Option<String>,
    theme: &Palette,
) -> gpui::Div {
    let MeterIds {
        row,
        value: value_selector,
        bar: bar_selector,
    } = ids;
    div()
        .debug_selector(move || row.clone())
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(theme.TEXT_DIM)
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .debug_selector(move || value_selector.clone())
                        .min_w_0()
                        .truncate()
                        .font_family(theme.FONT_MONO)
                        .child(value),
                ),
        )
        .children(bar.map(|(percent, ink)| usage_bar(bar_selector, percent, ink, theme)))
        .children(detail.map(|detail| meter_detail(detail, theme)))
}

pub(crate) fn meter_detail(detail: String, theme: &Palette) -> gpui::Div {
    div()
        .truncate()
        .text_size(px(10.))
        .text_color(theme.TEXT_FAINT)
        .child(detail)
}

/// Shared meter track and fill for context, quota and cache cards.
pub(crate) fn usage_bar(selector: String, percent: f32, ink: gpui::Rgba, theme: &Palette) -> gpui::Div {
    div()
        .debug_selector(move || selector.clone())
        .w_full()
        .h(px(4.))
        .rounded_full()
        .overflow_hidden()
        .bg(theme.INLINE_CODE_BG)
        .child(
            div()
                .h_full()
                .w(relative(percent.clamp(0., 100.) / 100.))
                .rounded_full()
                .bg(ink),
        )
}

