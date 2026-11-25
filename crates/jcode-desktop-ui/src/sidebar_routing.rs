//! Routing rows in the accounts sidebar: labelled API keys with their usage
//! windows, and budgets with used against limit. Fed by the routing view the
//! runtime reports; absent when the runtime has none.
use super::*;
use crate::routing::rows::{BarRow, BudgetRow, KeyRow, SidebarRouting, Tone};

fn tone_color(tone: Tone) -> gpui::Rgba {
    let theme = Theme::global();
    match tone {
        Tone::Accent => theme.ACCENT,
        Tone::Warn => theme.WARN,
        Tone::Error => theme.ERROR,
    }
}

fn section_heading(selector: &'static str, text: &'static str) -> gpui::Div {
    div()
        .debug_selector(move || selector.into())
        .flex_none()
        .px_4()
        .pt_2()
        .pb_1()
        .text_size(px(10.0))
        .text_color(Theme::global().TEXT_DIM)
        .child(text)
}

/// A caption over a bar. The caption is always drawn, the bar only when the
/// meter reads.
fn bar_row(selector: String, caption: String, bar: Option<(f32, Tone)>) -> gpui::Div {
    let theme = Theme::global();
    div()
        .debug_selector(move || selector.clone())
        .flex()
        .flex_col()
        .gap(px(2.0))
        .min_w_0()
        .child(
            div()
                .overflow_hidden()
                .h(px(12.0))
                .line_height(px(12.0))
                .text_size(px(8.0))
                .text_color(theme.TEXT_DIM)
                .whitespace_nowrap()
                .child(caption),
        )
        .children(bar.map(|(fraction, tone)| super::usage_bar(fraction, tone_color(tone))))
}

fn window_row(key: usize, index: usize, row: &BarRow) -> gpui::Div {
    bar_row(
        format!("routing-key-{key}-window-{index}"),
        row.label.clone(),
        row.fraction.map(|fraction| (fraction, row.tone)),
    )
}

fn key_block(index: usize, key: &KeyRow) -> gpui::Div {
    let theme = Theme::global();
    div()
        .debug_selector(move || format!("routing-key-{index}"))
        .flex_none()
        .mx_2()
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .overflow_hidden()
        .rounded_md()
        .hover(|el| el.bg(theme.HEADER_BG))
        .child(
            div()
                .flex()
                .items_center()
                .h(px(16.0))
                .line_height(px(16.0))
                .gap_2()
                .child(
                    div()
                        .debug_selector(move || format!("routing-key-{index}-title"))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(theme.TEXT)
                        .child(key.title.clone()),
                )
                .children(key.active.then(|| {
                    div()
                        .debug_selector(move || format!("routing-key-{index}-active"))
                        .flex_none()
                        .text_size(px(9.0))
                        .text_color(theme.ACCENT)
                        .child("active")
                })),
        )
        .children(
            key.windows
                .iter()
                .enumerate()
                .map(|(window, row)| window_row(index, window, row)),
        )
}

fn budget_block(index: usize, budget: &BudgetRow) -> gpui::Div {
    let theme = Theme::global();
    div()
        .debug_selector(move || format!("routing-budget-{index}"))
        .flex_none()
        .mx_2()
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .overflow_hidden()
        .rounded_md()
        .hover(|el| el.bg(theme.HEADER_BG))
        .child(
            div()
                .flex()
                .items_center()
                .h(px(16.0))
                .line_height(px(16.0))
                .gap_2()
                .child(
                    div()
                        .debug_selector(move || format!("routing-budget-{index}-name"))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(theme.TEXT)
                        .child(budget.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(9.0))
                        .text_color(theme.TEXT_DIM)
                        .child(budget.scope.clone()),
                ),
        )
        .child(bar_row(
            format!("routing-budget-{index}-usage"),
            budget.summary.clone(),
            budget.bar,
        ))
}

impl Workspace {
    /// Whether the routing section has anything to show.
    pub(super) fn has_routing_rows(&self) -> bool {
        self.routing_rows().is_some_and(|rows| !rows.is_empty())
    }

    fn routing_rows(&self) -> Option<SidebarRouting> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |now| now.as_secs() as i64);
        self.routing_view
            .as_ref()
            .map(|view| SidebarRouting::from_view(view, now))
    }

    /// Append the routing sections below the account rows.
    pub(super) fn append_routing_rows(
        &self,
        mut list: gpui::Stateful<gpui::Div>,
    ) -> gpui::Stateful<gpui::Div> {
        let Some(rows) = self.routing_rows().filter(|rows| !rows.is_empty()) else {
            return list;
        };
        if !rows.keys.is_empty() {
            list = list.child(section_heading("routing-keys-heading", "API keys"));
            for (index, key) in rows.keys.iter().enumerate() {
                list = list.child(key_block(index, key));
            }
        }
        if !rows.budgets.is_empty() {
            list = list.child(section_heading("routing-budgets-heading", "Budgets"));
            for (index, budget) in rows.budgets.iter().enumerate() {
                list = list.child(budget_block(index, budget));
            }
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            w.sidebar_view = SidebarView::Accounts;
            w.set_test_accounts(
                crate::accounts::parse(
                    r#"{"providers":[{"id":"claude","display_name":"Claude","status":"available","auth_kind":"OAuth"}]}"#,
                )
                .unwrap(),
            );
            w.set_test_routing_view(crate::routing::fixture());
            cx.notify();
        });
        vcx.run_until_parked();
        (workspace, vcx)
    }

    #[gpui::test]
    fn keys_and_budgets_render_below_the_accounts_inside_the_list(cx: &mut gpui::TestAppContext) {
        let (_workspace, vcx) = seeded(cx);
        let list = vcx.debug_bounds("accounts-list").expect("list");
        let account = vcx.debug_bounds("account-claude").expect("account row");
        let keys = vcx
            .debug_bounds("routing-keys-heading")
            .expect("keys heading");
        let budgets = vcx
            .debug_bounds("routing-budgets-heading")
            .expect("budgets heading");
        assert!(
            account.bottom() <= keys.top(),
            "routing rows follow the accounts"
        );
        assert!(keys.bottom() <= budgets.top());
        for selector in [
            "routing-key-0-title",
            "routing-key-1-title",
            "routing-key-1-active",
            "routing-key-1-window-0",
            "routing-key-1-window-1",
            "routing-budget-0-name",
            "routing-budget-0-usage",
            "routing-budget-2-usage",
        ] {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} paints"));
            assert!(
                bounds.left() >= list.left() && bounds.right() <= list.right(),
                "{selector} stays inside the list: {bounds:?} in {list:?}"
            );
        }
        assert!(
            vcx.debug_bounds("routing-key-0-active").is_none(),
            "only the active key is marked"
        );
    }

    #[gpui::test]
    fn an_unreadable_budget_has_a_caption_and_no_bar_while_a_read_one_has_both(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_workspace, vcx) = seeded(cx);
        let read = vcx.debug_bounds("routing-budget-0-usage").unwrap();
        let unknown = vcx.debug_bounds("routing-budget-2-usage").unwrap();
        assert!(
            read.size.height > unknown.size.height,
            "a bar adds height under the caption: {read:?} vs {unknown:?}"
        );
    }

    #[gpui::test]
    fn no_routing_view_leaves_the_accounts_list_as_it_was(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = seeded(cx);
        workspace.update(vcx, |w, cx| {
            w.routing_view = Some(Default::default());
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("account-claude").is_some());
        assert!(vcx.debug_bounds("routing-keys-heading").is_none());
        assert!(vcx.debug_bounds("routing-budgets-heading").is_none());
    }
}
