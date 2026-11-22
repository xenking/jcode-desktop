use super::*;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Sort {
    #[default]
    Recent,
    AgentActivity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OriginFilter {
    #[default]
    All,
    Mine,
    Agents,
}

impl OriginFilter {
    pub(super) fn matches(self, session: &jcode_sdk::SessionInfo) -> bool {
        match self {
            Self::All => true,
            Self::Mine => !session.is_agent_started(),
            Self::Agents => session.is_agent_started(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub(super) sort: Sort,
    pub(super) origin: OriginFilter,
}

fn pill(
    surface: &'static str,
    id: &'static str,
    label: &'static str,
    tooltip: &'static str,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    crate::keyboard::button(div().id(gpui::SharedString::from(format!("{surface}-{id}"))))
        .tab_index(3)
        .relative()
        .debug_selector(move || format!("{surface}-{id}"))
        .children(crate::fixture_control_targets::marker(
            || format!("{surface}-{id}"),
            || None,
        ))
        .min_w_0()
        .px(px(if surface == "sidebar" { 2.0 } else { 4.0 }))
        .h(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(if selected { theme.TEXT } else { theme.TEXT_DIM })
        .when(selected, |el| el.bg(theme.TOOL_BG))
        .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
        .tooltip(move |_, cx| cx.new(|_| remotes::HeaderTooltip(tooltip.into())).into())
        .child(
            div()
                .min_w_0()
                .truncate()
                .debug_selector(move || {
                    if selected {
                        format!("{surface}-{id}-selected")
                    } else {
                        format!("{surface}-{id}-label")
                    }
                })
                .child(label),
        )
}

pub(super) fn agent_badge() -> gpui::Div {
    div()
        .flex_none()
        .text_size(px(10.0))
        .text_color(Theme::global().TEXT_DIM)
        .whitespace_nowrap()
        .child("Agent")
}

impl Workspace {
    pub(super) fn session_catalog_controls(
        &self,
        surface: &'static str,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let theme = Theme::global();
        let selected = self.session_catalog;
        div()
            .flex_none()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(if surface == "sidebar" { 4.0 } else { 8.0 }))
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .rounded_full()
                    .bg(theme.PANEL_BG)
                    .children(
                        [
                            (Sort::Recent, "sort-recent", "Recent", "Use the usual order"),
                            (
                                Sort::AgentActivity,
                                "sort-agent-activity",
                                if surface == "sidebar" {
                                    "Replies"
                                } else {
                                    "Agent activity"
                                },
                                "Latest assistant reply first",
                            ),
                        ]
                        .map(|(value, id, label, tooltip)| {
                            pill(surface, id, label, tooltip, selected.sort == value).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.session_catalog.sort = value;
                                    cx.notify();
                                }),
                            )
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .rounded_full()
                    .bg(theme.PANEL_BG)
                    .children(
                        [
                            (
                                OriginFilter::All,
                                "origin-all",
                                "All",
                                "Show all session origins",
                            ),
                            (
                                OriginFilter::Mine,
                                "origin-mine",
                                "Mine",
                                "Show user-started sessions",
                            ),
                            (
                                OriginFilter::Agents,
                                "origin-agents",
                                "Agents",
                                "Show agent-started sessions",
                            ),
                        ]
                        .map(|(value, id, label, tooltip)| {
                            pill(surface, id, label, tooltip, selected.origin == value).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.session_catalog.origin = value;
                                    cx.notify();
                                }),
                            )
                        }),
                    ),
            )
    }
}
