use super::{Panel, composer, usage, *};
use jcode_desktop_openrig::fleet::Fleet;

#[derive(Clone, PartialEq)]
pub(super) struct Identity {
    action: crate::ui_actions::InspectFleetSeat,
    agent: String,
    rig: String,
    pod: Option<String>,
    host: String,
    status: Option<String>,
    activity: Option<String>,
    work: Option<u64>,
}

pub(super) fn identify(
    fleet: &Fleet,
    session: &str,
    daemon_host_id: Option<&str>,
) -> Option<Identity> {
    if session.starts_with("cloud://") {
        return None;
    }
    let (_, native) = crate::harness::session_address(session)?;
    let (h, r, p, s) = fleet.jcode_seat(daemon_host_id?, &native)?;
    Some(Identity {
        action: crate::ui_actions::InspectFleetSeat {
            host_id: h.id.clone(),
            rig_id: r.id.clone(),
            seat_id: s.id.clone(),
        },
        agent: s.logical_id.clone(),
        rig: r.name.clone(),
        pod: p.name.clone().or_else(|| p.id.clone()),
        host: h.name.clone(),
        status: s.status.clone(),
        activity: s.activity.clone(),
        work: s.current_work_count,
    })
}

fn status_label(status: &str) -> &str {
    match status {
        "attention_required" => "Needs attention",
        other => other,
    }
}

fn in_progress_label(count: u64) -> String {
    format!("{count} {}", if count == 1 { "item" } else { "items" })
}

struct Tooltip {
    panel: gpui::WeakEntity<Panel>,
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(panel) = self.panel.upgrade() else {
            return div();
        };
        let panel = panel.read(cx);
        let Some(identity) = panel.rig_identity.clone() else {
            return div();
        };
        let theme = Theme::global();
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .justify_between()
                .gap_3()
                .child(div().text_color(theme.TEXT_FAINT).child(label))
                .child(div().text_color(theme.TEXT).child(value))
        };
        usage::meter_card(theme)
            .relative()
            .children(crate::fixture_control_targets::marker(
                || "tooltip:openrig".into(),
                || None,
            ))
            .debug_selector(|| "panel-openrig-tooltip".into())
            .w(px(312.))
            .child(
                div()
                    .text_color(theme.TEXT)
                    .font_weight(FontWeight::MEDIUM)
                    .child("OpenRig seat"),
            )
            .child(row("Agent", identity.agent))
            .child(row("Rig", identity.rig))
            .children(identity.pod.map(|v| row("Pod", v)))
            .child(row("Host", identity.host))
            .children(
                identity
                    .status
                    .map(|v| row("Status", status_label(&v).to_owned())),
            )
            .children(identity.activity.map(|v| row("Activity", v)))
            .child(row("Runtime", "Jcode".into()))
            .children(panel.model.as_ref().map(|v| row("Model", v.clone())))
            .children(
                identity
                    .work
                    .map(|v| row("In progress", in_progress_label(v))),
            )
            .child(usage::meter_detail(
                "Click to open in OpenRig".into(),
                theme,
            ))
    }
}

impl Panel {
    pub(crate) fn set_rig_host_id(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        if self.rig_host_id == host {
            return;
        }
        self.rig_demand = host
            .clone()
            .map(|host| self.rig_feed.read(cx).demand(Some(host)));
        self.rig_host_id = host;
        self.refresh_rig_hud(cx);
    }

    pub(super) fn refresh_rig_hud(&mut self, cx: &mut Context<Self>) {
        let identity = self.rig_host_id.as_deref().and_then(|host| {
            identify(
                self.rig_feed.read(cx).fleet()?,
                &self.session_id,
                Some(host),
            )
        });
        if self.rig_identity != identity {
            self.rig_identity = identity;
            cx.notify();
        }
    }

    pub(super) fn render_rig_hud(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let action = self.rig_identity.as_ref()?.action.clone();
        let panel = cx.entity().downgrade();
        let theme = Theme::global();
        let selector = format!("panel-openrig:{}", self.session_id);
        Some(
            composer::composer_pill("panel-openrig")
                .relative()
                .flex_none()
                .px_2()
                .debug_selector(|| "panel-openrig".into())
                .children(crate::fixture_control_targets::marker(
                    || "hover:panel-openrig".into(),
                    || Some(self.session_id.clone()),
                ))
                .child(
                    div().debug_selector(move || selector.clone()).child(
                        crate::swarm_glyph::Mark::still(
                            crate::swarm_glyph::Icon::OpenRig,
                            theme.composer_secondary(),
                            composer::BAR_ICON,
                        )
                        .render(None),
                    ),
                )
                .tooltip(move |_, cx| {
                    cx.new(|cx| {
                        if let Some(panel) = panel.upgrade() {
                            cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                        }
                        Tooltip {
                            panel: panel.clone(),
                        }
                    })
                    .into()
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this
                        .rig_identity
                        .as_ref()
                        .is_some_and(|current| current.action == action)
                    {
                        window.dispatch_action(Box::new(action.clone()), cx);
                    }
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_measured_status_and_work_counts() {
        assert_eq!(status_label("attention_required"), "Needs attention");
        assert_eq!(status_label("unrecognized"), "unrecognized");
        assert_eq!(in_progress_label(0), "0 items");
        assert_eq!(in_progress_label(1), "1 item");
        assert_eq!(in_progress_label(2), "2 items");
    }

    #[test]
    fn only_exact_native_session_on_exact_host_gets_identity() {
        let fleet = Fleet::fixture();
        let id = "session_fox_1700000000000_0123456789abcdef";
        let local = identify(&fleet, id, Some("host-fixture-local")).unwrap();
        assert_eq!(local.action.host_id, "host-fixture-local");
        let remote = identify(
            &fleet,
            &format!("ssh://worker-east.example.test/{id}"),
            Some("host-fixture-remote"),
        )
        .unwrap();
        assert_eq!(remote.action.host_id, "worker-east");
        assert!(identify(&fleet, "ordinary-session", None).is_none());
        assert!(identify(&fleet, &format!("ssh://other.example.test/{id}"), None).is_none());
        assert!(
            identify(
                &fleet,
                &format!("ssh://worker-east.example.test/{id}"),
                Some("wrong-host")
            )
            .is_none()
        );
        let mut duplicate = fleet.clone();
        duplicate.hosts.push(duplicate.hosts[1].clone());
        assert!(
            identify(
                &duplicate,
                &format!("ssh://worker-east.example.test/{id}"),
                Some("host-fixture-remote")
            )
            .is_none()
        );
        let mut missing_identity = fleet.clone();
        missing_identity.hosts[0].self_host_id = None;
        assert!(identify(&missing_identity, id, Some("host-fixture-local")).is_none());
        assert!(identify(&fleet, id, None).is_none());
        assert!(
            identify(
                &fleet,
                &format!("cloud://worker-east.example.test/{id}"),
                Some("host-fixture-remote")
            )
            .is_none()
        );
    }
}
