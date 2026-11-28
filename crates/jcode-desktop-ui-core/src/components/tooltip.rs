use gpui::prelude::*;
use gpui::{AnyView, App, Context, Pixels, SharedString, Window, div, px};

use crate::theme::Theme;

#[derive(Clone)]
enum Content {
    Text(SharedString),
    HeaderText(SharedString),
    Rows {
        title: SharedString,
        rows: Vec<SharedString>,
    },
}

/// The one hover view. Replaces the per-feature `*Tooltip` structs that only
/// show text or rows.
///
/// ```ignore
/// div().tooltip(Tooltip::text("Hide sidebar").key_hint("Ctrl+B").builder())
/// ```
#[derive(Clone)]
pub struct Tooltip {
    content: Content,
    key_hint: Option<SharedString>,
    max_width: Option<Pixels>,
    selector: Option<&'static str>,
}

impl Tooltip {
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self::with(Content::Text(text.into()))
    }

    pub fn header_text(text: impl Into<SharedString>) -> Self {
        Self::with(Content::HeaderText(text.into())).max_width(px(300.))
    }

    /// A title over a few dim rows, for controls with several facts.
    pub fn rows(
        title: impl Into<SharedString>,
        rows: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self::with(Content::Rows {
            title: title.into(),
            rows: rows.into_iter().map(Into::into).collect(),
        })
    }

    fn with(content: Content) -> Self {
        Self {
            content,
            key_hint: None,
            max_width: None,
            selector: None,
        }
    }

    /// The keyboard shortcut for the action, shown after the text.
    pub fn key_hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.key_hint = Some(hint.into());
        self
    }

    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Test hook. Rows tooltips also get `{selector}-title` and
    /// `{selector}-row-{index}`.
    pub fn debug_selector(mut self, selector: &'static str) -> Self {
        self.selector = Some(selector);
        self
    }

    pub fn key_hint_text(&self) -> Option<SharedString> {
        self.key_hint.clone()
    }

    /// The closure GPUI's `.tooltip(...)` takes. It builds a fresh view per
    /// hover.
    pub fn builder(self) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        move |_, cx| cx.new(|_| self.clone()).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("Tooltip");
        let theme = Theme::global();
        let selector = self.selector;
        match &self.content {
            Content::HeaderText(text) => div()
                .when_some(selector, |el, selector| {
                    el.debug_selector(move || selector.into())
                })
                .when_some(self.max_width, |el, width| el.max_w(width))
                .p_2()
                .rounded_md()
                .bg(theme.HEADER_BG)
                .text_size(px(12.))
                .text_color(theme.TEXT)
                .child(text.clone())
                .into_any_element(),
            Content::Text(text) => div()
                .when_some(selector, |el, selector| {
                    el.debug_selector(move || selector.into())
                })
                .when_some(self.max_width, |el, width| el.max_w(width))
                .px_2()
                .py_1()
                .rounded_md()
                .bg(theme.PANEL_BG)
                .border_1()
                .border_color(theme.PANEL_BORDER)
                .text_size(px(12.0))
                .text_color(theme.TEXT)
                .when(self.key_hint.is_some(), |el| {
                    el.flex().items_center().gap_1()
                })
                .child(text.clone())
                .when_some(self.key_hint.clone(), |el, hint| {
                    el.child(div().text_color(theme.TEXT_DIM).child(format!("({hint})")))
                })
                .into_any_element(),
            Content::Rows { title, rows } => div()
                .when_some(selector, |el, selector| {
                    el.debug_selector(move || selector.into())
                })
                .px_3()
                .py_2()
                .min_w(px(180.))
                .max_w(self.max_width.unwrap_or(px(320.)))
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
                        .when_some(selector, |el, selector| {
                            el.debug_selector(move || format!("{selector}-title"))
                        })
                        .whitespace_normal()
                        .text_color(theme.TEXT)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title.clone()),
                )
                .children(rows.iter().enumerate().map(|(index, row)| {
                    div()
                        .when_some(selector, |el, selector| {
                            el.debug_selector(move || format!("{selector}-row-{index}"))
                        })
                        .flex_none()
                        .line_height(px(16.))
                        .whitespace_normal()
                        .text_color(theme.TEXT_DIM)
                        .child(row.clone())
                }))
                .into_any_element(),
        }
    }
}
