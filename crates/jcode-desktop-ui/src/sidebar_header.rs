use super::*;

pub(super) struct WorkersTooltip(pub Vec<crate::swarm_view::Worker>);

impl Render for WorkersTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .debug_selector(|| "sidebar-workers-tooltip".into())
            .px_3()
            .py_2()
            .min_w(px(180.0))
            .max_w(px(320.0))
            .rounded_xl()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(11.0))
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.TEXT)
                    .child("Workers"),
            )
            .children(self.0.iter().enumerate().map(|(index, worker)| {
                div()
                    .debug_selector(move || format!("sidebar-workers-tooltip-row-{index}"))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(theme.TEXT_DIM)
                    .child(worker.name.clone())
                    .child("·")
                    .child(
                        worker
                            .model_label()
                            .unwrap_or_else(|| "Unknown model".into()),
                    )
                    .child("·")
                    .child(div().text_color(theme.OK).child("●"))
            }))
    }
}
