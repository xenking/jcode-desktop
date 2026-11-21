use super::{Panel, composer, usage, *};
use jcode_desktop_openrig::fleet::Fleet;

#[derive(Clone)]
struct Identity {
    action: crate::ui_actions::InspectFleetSeat,
    agent: String,
    rig: String,
    pod: Option<String>,
    host: String,
    status: Option<String>,
    activity: Option<String>,
    work: Option<u64>,
}

fn identify(fleet: &Fleet, session: &str, daemon_host_id: Option<&str>) -> Option<Identity> {
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

struct Tooltip {
    panel: gpui::WeakEntity<Panel>,
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(panel) = self.panel.upgrade() else {
            return div();
        };
        let panel = panel.read(cx);
        let Some(identity) = panel
            .rig_feed
            .read(cx)
            .snapshot
            .as_ref()
            .and_then(|s| identify(&s.fleet, &panel.session_id, panel.rig_host_id.as_deref()))
        else {
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
            .children(identity.status.map(|v| row("Status", v)))
            .children(identity.activity.map(|v| row("Activity", v)))
            .child(row("Runtime", "Jcode".into()))
            .children(panel.model.as_ref().map(|v| row("Model", v.clone())))
            .children(identity.work.map(|v| row("Current work", v.to_string())))
            .child(usage::meter_detail(
                "Click to open in OpenRig".into(),
                theme,
            ))
    }
}

impl Panel {
    pub(super) fn render_rig_hud(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let identity = identify(
            &self.rig_feed.read(cx).snapshot.as_ref()?.fleet,
            &self.session_id,
            self.rig_host_id.as_deref(),
        )?;
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
                        .rig_feed
                        .read(cx)
                        .snapshot
                        .as_ref()
                        .and_then(|s| {
                            identify(&s.fleet, &this.session_id, this.rig_host_id.as_deref())
                        })
                        .is_some_and(|current| current.action == identity.action)
                    {
                        window.dispatch_action(Box::new(identity.action.clone()), cx);
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
