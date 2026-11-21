//! Fleet outline and seat properties for the OpenRig panel.

use super::{Panel, State, tone_color};
use crate::{theme::Theme, ui_actions::OpenFleetSession};
use gpui::{Context, FontWeight, MouseButton, ScrollHandle, SharedString, div, prelude::*, px};
use jcode_desktop_openrig::{
    Tone,
    fleet::{Fleet, Host, Pod, Rig, Runtime, Seat},
};
use std::collections::HashSet;

const UNKNOWN: &str = "–";

#[derive(Default)]
pub(super) struct FleetView {
    selected: Option<String>,
    collapsed: HashSet<String>,
    pub(super) properties: bool,
    scroll: ScrollHandle,
}

#[derive(Clone, Copy)]
enum Item<'a> {
    Host(&'a Host),
    Rig(&'a Host, &'a Rig),
    Pod(&'a Host, &'a Rig, &'a Pod),
    Seat(&'a Host, &'a Rig, &'a Pod, &'a Seat),
}

impl<'a> Item<'a> {
    fn key(self) -> String {
        match self {
            Self::Host(host) => format!("host:{:?}", host.id),
            Self::Rig(host, rig) => format!("rig:{:?}/{:?}", host.id, rig.id),
            Self::Pod(host, rig, pod) => format!(
                "pod:{:?}/{:?}/{:?}",
                host.id,
                rig.id,
                pod.id.as_ref().or(pod.name.as_ref())
            ),
            Self::Seat(host, rig, _, seat) => {
                format!("seat:{:?}/{:?}/{:?}", host.id, rig.id, seat.id)
            }
        }
    }

    fn label(self) -> &'a str {
        match self {
            Self::Host(host) => &host.name,
            Self::Rig(_, rig) => &rig.name,
            Self::Pod(_, _, pod) => pod
                .name
                .as_deref()
                .or(pod.id.as_deref())
                .unwrap_or("Unassigned"),
            Self::Seat(_, _, _, seat) => &seat.logical_id,
        }
    }

    fn status(self) -> Option<&'a str> {
        match self {
            Self::Host(host) => host.status.as_deref(),
            Self::Rig(_, rig) => rig.status.as_deref(),
            Self::Pod(..) => None,
            Self::Seat(_, _, _, seat) => seat.status.as_deref(),
        }
    }

    fn meta(self) -> &'a str {
        match self {
            Self::Host(Host {
                rigs: None, error, ..
            }) => error.as_deref().unwrap_or("Rigs unavailable"),
            Self::Host(host) if host.target.is_local_machine() => "This machine",
            Self::Host(_) => "Remote host",
            Self::Rig(_, Rig { pods: None, .. }) => "Seats unavailable",
            Self::Rig(..) => "Rig",
            Self::Pod(..) => "Pod",
            Self::Seat(_, _, _, seat) => seat.runtime.as_ref().map_or(UNKNOWN, Runtime::label),
        }
    }

    fn depth(self) -> u8 {
        match self {
            Self::Host(..) => 0,
            Self::Rig(..) => 1,
            Self::Pod(..) => 2,
            Self::Seat(..) => 3,
        }
    }

    fn has_children(self) -> bool {
        match self {
            Self::Host(host) => host.rigs.as_ref().is_some_and(|rigs| !rigs.is_empty()),
            Self::Rig(_, rig) => rig.pods.as_ref().is_some_and(|pods| !pods.is_empty()),
            Self::Pod(_, _, pod) => !pod.seats.is_empty(),
            Self::Seat(..) => false,
        }
    }

    fn session(self) -> Option<OpenFleetSession> {
        let Self::Seat(host, _, _, seat) = self else {
            return None;
        };
        if seat.runtime != Some(Runtime::Jcode) {
            return None;
        }
        Some(OpenFleetSession {
            target: host.target.clone(),
            native_id: seat.jcode_session_id.clone()?,
        })
    }
}

impl FleetView {
    pub(super) fn fixture(properties: bool) -> Self {
        let fleet = Fleet::fixture();
        let selected = properties.then(|| {
            let host = &fleet.hosts[0];
            let rig = &host.rigs.as_ref().unwrap()[0];
            let pod = rig
                .pods
                .as_ref()
                .unwrap()
                .iter()
                .find(|pod| pod.name.as_deref() == Some("operator"))
                .unwrap();
            let seat = pod
                .seats
                .iter()
                .find(|seat| seat.logical_id == "operator.agent")
                .unwrap();
            Item::Seat(host, rig, pod, seat).key()
        });
        Self {
            selected,
            properties,
            ..Default::default()
        }
    }

    fn visible<'a>(&self, fleet: &'a Fleet, filter: &str) -> Vec<Item<'a>> {
        let query = filter.to_lowercase();
        let mut items = Vec::new();
        let is_open = |item: Item| !query.is_empty() || !self.collapsed.contains(&item.key());
        for host in &fleet.hosts {
            let h = Item::Host(host);
            let host_at = items.len();
            items.push(h);
            if is_open(h) {
                for rig in host.rigs.as_ref().into_iter().flatten() {
                    let r = Item::Rig(host, rig);
                    let rig_at = items.len();
                    items.push(r);
                    if is_open(r) {
                        for pod in rig.pods.as_ref().into_iter().flatten() {
                            let p = Item::Pod(host, rig, pod);
                            let pod_at = items.len();
                            items.push(p);
                            let ancestor_match = [h, r, p]
                                .iter()
                                .any(|item| item.label().to_lowercase().contains(&query));
                            if is_open(p) {
                                items.extend(
                                    pod.seats
                                        .iter()
                                        .map(|seat| Item::Seat(host, rig, pod, seat))
                                        .filter(|item| {
                                            ancestor_match
                                                || item.label().to_lowercase().contains(&query)
                                                || item.meta().to_lowercase().contains(&query)
                                        }),
                                );
                            }
                            if !ancestor_match && items.len() == pod_at + 1 {
                                items.pop();
                            }
                        }
                    }
                    if items.len() == rig_at + 1
                        && ![h, r]
                            .iter()
                            .any(|item| item.label().to_lowercase().contains(&query))
                    {
                        items.pop();
                    }
                }
            }
            if items.len() == host_at + 1 && !h.label().to_lowercase().contains(&query) {
                items.pop();
            }
        }
        items
    }

    fn selected(&self, items: &[Item<'_>]) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        items.iter().position(|item| &item.key() == selected)
    }

    fn toggle(&mut self, key: String) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
    }

    pub(super) fn key(
        &mut self,
        fleet: &Fleet,
        filter: &str,
        key: &str,
    ) -> Option<OpenFleetSession> {
        let items = self.visible(fleet, filter);
        let selected = self.selected(&items);
        match key {
            "j" | "down" | "k" | "up" if !items.is_empty() => {
                let upper = matches!(key, "k" | "up");
                let index = selected.map_or(0, |index| {
                    if upper {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(items.len() - 1)
                    }
                });
                self.selected = Some(items[index].key());
                self.scroll.scroll_to_item(index);
            }
            "enter" | "right" => {
                let item = items.get(selected.unwrap_or(0)).copied()?;
                self.selected = Some(item.key());
                if item.has_children() {
                    self.toggle(item.key());
                }
                if matches!(item, Item::Seat(..)) {
                    self.properties = true;
                    if key == "enter" {
                        return item.session();
                    }
                }
            }
            _ => {}
        }
        None
    }
}

fn status_color(theme: &Theme, status: &str) -> gpui::Rgba {
    tone_color(
        theme,
        match status {
            "ok" | "reachable" | "active" | "running" | "ready" => Tone::Ok,
            "failed" | "unreachable" => Tone::Error,
            "attention_required" | "needs_input" => Tone::Warn,
            _ => Tone::Dim,
        },
    )
}

fn status_text(status: &str) -> String {
    if status == "unknown" {
        return UNKNOWN.into();
    }
    let mut label = status.replace(['_', '-'], " ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}

impl Panel {
    pub(super) fn render_fleet(&self, state: &State, cx: &Context<Self>) -> gpui::Div {
        let theme = Theme::global();
        let fleet = &state.snapshot.fleet;
        let items = state.fleet.visible(fleet, &state.filter);
        let selected = state.fleet.selected(&items);
        let seat_item = selected
            .and_then(|index| items.get(index))
            .copied()
            .filter(|item| matches!(item, Item::Seat(..)));
        let show_properties = state.fleet.properties && seat_item.is_some();
        let mut list = div()
            .id("fleet-outline")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&state.fleet.scroll)
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .pb_3();
        let filtered = !state.filter.is_empty();
        for (index, item) in items.iter().copied().enumerate() {
            let key = item.key();
            let is_seat = matches!(item, Item::Seat(..));
            let has_children = item.has_children() && !filtered;
            let marker = if has_children {
                if state.fleet.collapsed.contains(&key) {
                    "▸"
                } else {
                    "▾"
                }
            } else {
                ""
            };
            let active = selected == Some(index);
            let target = item.session();
            let row = div()
                .id(SharedString::from(key.clone()))
                .rounded_full()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(29.))
                .pl(px(10. + 12. * f32::from(item.depth())))
                .pr_3()
                .cursor_pointer()
                .bg(if active {
                    theme.ACCENT_DIM
                } else {
                    theme.HEADER_BG
                })
                .hover(|style| style.bg(theme.ACCENT_DIM))
                .debug_selector({
                    let key = key.clone();
                    move || key.clone()
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |panel, event: &gpui::MouseDownEvent, window, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.fleet.selected = Some(key.clone());
                            state.explorer = false;
                            if is_seat {
                                state.fleet.properties = true;
                            }
                            if has_children && event.click_count == 1 {
                                state.fleet.toggle(key.clone());
                            }
                        }
                        window.focus(&panel.focus_handle, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_up(MouseButton::Left, move |event, window, cx| {
                    if event.click_count == 2
                        && let Some(target) = &target
                    {
                        window.dispatch_action(Box::new(target.clone()), cx);
                    }
                })
                .child(
                    div()
                        .w(px(10.))
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(marker),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme.TEXT)
                        .font_weight(if is_seat {
                            FontWeight::NORMAL
                        } else {
                            FontWeight::SEMIBOLD
                        })
                        .child(item.label().to_owned()),
                )
                .child(
                    div()
                        .max_w(px(if state.compact { 82. } else { 125. }))
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(item.meta().to_owned()),
                );
            let row = if let Some(status) = item.status().or(is_seat.then_some(UNKNOWN)) {
                row.child(
                    div()
                        .max_w(px(if state.compact { 70. } else { 125. }))
                        .truncate()
                        .text_size(px(11.))
                        .text_color(status_color(theme, status))
                        .child(status_text(status)),
                )
            } else {
                row
            };
            list = list.child(row);
        }
        if items.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_3()
                    .text_size(px(12.))
                    .text_color(theme.TEXT_DIM)
                    .child(if filtered {
                        "No fleet entries match your filter."
                    } else if state.snapshot.error.is_some() {
                        "Fleet inventory is unavailable."
                    } else if state.snapshot.sections.is_empty() {
                        "Loading fleet…"
                    } else {
                        "No hosts reported."
                    }),
            );
        }
        let title = div()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(17.))
                    .text_color(theme.TEXT)
                    .child("Fleet"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_DIM)
                    .child("Hosts, rigs, pods, and seats"),
            );
        let outline = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(title)
            .child(list);
        let mut body = div().flex().flex_1().min_w_0().min_h_0().overflow_hidden();
        if !state.compact || !show_properties {
            body = body.child(outline);
        }
        if show_properties && let Some(Item::Seat(host, rig, pod, seat)) = seat_item {
            body = body.child(self.render_seat_properties(state, host, rig, pod, seat, cx));
        }
        body
    }

    fn render_seat_properties(
        &self,
        state: &State,
        host: &Host,
        rig: &Rig,
        pod: &Pod,
        seat: &Seat,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::global();
        let session = seat.jcode_session_id.as_deref();
        let context = seat.context_percent.map(|p| format!("{p:.0}%"));
        let work = seat.current_work_count.map(|count| match count {
            0 => "None".into(),
            1 => "1 item in progress".into(),
            count => format!("{count} items in progress"),
        });
        let key_value = |label: &'static str, text: Option<&str>| {
            div()
                .flex()
                .gap_2()
                .items_start()
                .child(
                    div()
                        .w(px(85.))
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(theme.TEXT)
                        .text_size(px(12.))
                        .child(text.unwrap_or(UNKNOWN).to_owned()),
                )
        };
        let mut properties = div()
            .id("fleet-properties")
            .debug_selector(|| "fleet-properties".into())
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .overflow_y_scroll();
        properties = if state.compact {
            properties.flex_1()
        } else {
            properties.w(px(278.)).flex_none()
        };
        if state.compact {
            properties = properties.child(
                div()
                    .id("fleet-back")
                    .debug_selector(|| "fleet-back".into())
                    .self_start()
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .text_size(px(11.))
                    .bg(theme.HEADER_BG)
                    .cursor_pointer()
                    .on_click(cx.listener(|panel, _, _, cx| {
                        if let Some(state) = panel.openrig.as_mut() {
                            state.fleet.properties = false;
                        }
                        cx.notify();
                    }))
                    .child("‹ Fleet"),
            );
        }
        let status = seat.status.as_deref().map(status_text);
        properties = properties
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_DIM)
                    .child("Seat properties"),
            )
            .child(
                div()
                    .text_color(theme.TEXT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(16.))
                    .child(seat.logical_id.clone()),
            )
            .child(key_value("Status", status.as_deref()))
            .child(key_value(
                "Runtime",
                seat.runtime.as_ref().map(Runtime::label),
            ))
            .child(key_value("Rig", Some(&rig.name)))
            .child(key_value("Pod", pod.name.as_deref().or(pod.id.as_deref())))
            .child(key_value("Session", session))
            .child(key_value("Context used", context.as_deref()))
            .child(key_value("Current work", work.as_deref()));
        if let Some(action) = Item::Seat(host, rig, pod, seat).session() {
            properties = properties.child(
                div()
                    .id("fleet-open-session")
                    .debug_selector(|| "fleet-open-session".into())
                    .self_start()
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .bg(theme.ACCENT_DIM)
                    .text_color(theme.TEXT)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(action.clone()), cx)
                    })
                    .child("Open Jcode session"),
            );
        }
        properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_activates_the_selected_jcode_seat_without_routing_other_rows() {
        let fleet = Fleet::fixture();
        let mut view = FleetView::default();
        let items = view.visible(&fleet, "");
        for (label, expected) in [
            ("operator.agent", true),
            ("operator.human", false),
            ("advisor.lead", false),
        ] {
            view.selected = Some(
                items
                    .iter()
                    .find(|item| item.label() == label)
                    .unwrap()
                    .key(),
            );
            assert_eq!(view.key(&fleet, "", "enter").is_some(), expected);
            assert!(view.properties);
        }
    }

    #[test]
    fn filter_retains_ancestry_and_stable_selection_after_inventory_reorders() {
        let mut fleet = Fleet::fixture();
        let mut view = FleetView::default();
        let query = "operator.agent";
        let filtered = view.visible(&fleet, query);
        assert_eq!(
            filtered
                .iter()
                .take(4)
                .map(|item| item.label())
                .collect::<Vec<_>>(),
            ["localhost", "kernel", "operator", "operator.agent"]
        );
        view.selected = Some(filtered[3].key());
        fleet.hosts.reverse();
        assert!(
            view.key(&fleet, query, "enter")
                .unwrap()
                .target
                .is_local_machine()
        );
    }
}
