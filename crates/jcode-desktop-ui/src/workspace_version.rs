//! Workspace-wide version and updater status in the bottom chrome.
use super::*;
use crate::updates::{ReleaseStatus, UpdateState};

fn status_label(
    release: &ReleaseStatus,
    update: &UpdateState,
    source: bool,
) -> (String, Option<&'static str>) {
    match update {
        UpdateState::Checking => ("Updating…".into(), None),
        UpdateState::Available { version } => (format!("Downloading {version}…"), None),
        UpdateState::ReadyToRestart { .. } => ("Update ready".into(), Some("Restart")),
        UpdateState::Finished { .. } => ("Update result".into(), None),
        UpdateState::Failed { .. } => ("Update failed".into(), Some("Retry")),
        UpdateState::Idle => match release {
            ReleaseStatus::Unknown | ReleaseStatus::Checking => {
                ("Checking for updates…".into(), None)
            }
            ReleaseStatus::Current { .. } => ("Latest version".into(), None),
            // A source checkout cannot install a release in place.
            ReleaseStatus::Newer { version } if source => (format!("{version} released"), None),
            ReleaseStatus::Newer { version } => (format!("{version} available"), Some("Update")),
            ReleaseStatus::Error { .. } => ("Unable to check".into(), None),
            ReleaseStatus::Source => ("Development build".into(), None),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tone {
    Busy,
    Good,
    News,
    Bad,
}

/// What a check the user just asked for is doing, shown beside the version.
/// `None` when the Restart or Update action already says it.
fn check_feedback(release: &ReleaseStatus, update: &UpdateState) -> Option<(String, Tone)> {
    Some(match update {
        UpdateState::Checking => ("Checking…".into(), Tone::Busy),
        UpdateState::Available { version } => (format!("Downloading {version}…"), Tone::Busy),
        UpdateState::ReadyToRestart { .. } => return None,
        UpdateState::Failed { .. } => ("Update failed".into(), Tone::Bad),
        UpdateState::Finished { .. } | UpdateState::Idle => match release {
            ReleaseStatus::Unknown | ReleaseStatus::Checking => ("Checking…".into(), Tone::Busy),
            ReleaseStatus::Current { .. } => ("Up to date".into(), Tone::Good),
            ReleaseStatus::Newer { version } => (format!("v{version} out"), Tone::News),
            ReleaseStatus::Error { .. } => ("Couldn't check".into(), Tone::Bad),
            ReleaseStatus::Source => return None,
        },
    })
}

fn visible_feedback() -> Option<(String, Tone)> {
    updates::user_check_visible()
        .then(|| check_feedback(&updates::release_status(), &updates::current()))
        .flatten()
}

/// The sidebar update item only appears while it offers something to do: Update,
/// Restart, or Retry. Otherwise the version lives in Settings → About and the
/// strip's height goes back to the sessions.
pub(super) fn footer_visible() -> bool {
    status_label(
        &updates::release_status(),
        &updates::current(),
        updates::is_source_build(),
    )
    .1
    .is_some()
}

/// Height the footer reserves below the session canvas.
/// The strip now lives at the bottom of the sidebar, so the session canvas
/// never reserves height for it.
pub(super) fn footer_height() -> f32 {
    0.0
}

impl Workspace {
    /// Clicking the version pill: check for a newer release right away and
    /// ask the platform updater to check, download, or install whatever is
    /// next. The pill shows the check and its result.
    fn check_and_apply_update(cx: &mut gpui::App) {
        updates::note_user_check();
        // A newer release this build cannot install itself (ad-hoc package
        // without Sparkle keys): open the fork's release page instead of
        // reporting a failed install.
        if let ReleaseStatus::Newer { version } = updates::release_status()
            && !updates::installer_available()
        {
            cx.open_url(&updates::release_page(&version));
            return;
        }
        updates::check_release_now();
        // A source checkout only reports releases. Ctrl+R or /update rebuild it.
        if updates::is_source_build() || !updates::installer_available() {
            return;
        }
        if updates::request_now() == updates::UpdateRequest::Unavailable {
            updates::set(UpdateState::Failed {
                message: "Automatic updates are unavailable in this build. Install the latest Desktop release manually.".into(),
            });
        }
    }

    /// Settings → About: the version, its release status, and a manual check.
    pub(super) fn render_about_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        updates::ensure_release_check();
        let release = updates::release_status();
        let update = updates::current();
        let (label, action) = status_label(&release, &update, updates::is_source_build());
        let theme = Theme::global();
        let feedback = visible_feedback();
        div()
            .id("settings-about")
            .debug_selector(|| "settings-about".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_color(theme.TEXT_DIM).child("About"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(theme.PANEL_BG)
                    .child(
                        div()
                            .id("settings-about-version")
                            .debug_selector(|| "settings-about-version".into())
                            .flex_none()
                            .font_family(theme.FONT_MONO)
                            .text_color(theme.TEXT)
                            .tooltip(|_, cx| cx.new(|_| crate::build_info::BuildTooltip).into())
                            .child(crate::build_info::version()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-about-status".into())
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.TEXT_DIM)
                            .child(feedback.map_or(label, |(text, _)| text)),
                    )
                    .child(
                        div()
                            .id("settings-check-updates")
                            .debug_selector(|| "settings-check-updates".into())
                            .flex_none()
                            .px_2()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(theme.ACCENT.opacity(0.12))
                            .text_color(theme.ACCENT)
                            .hover(|el| el.bg(theme.ACCENT.opacity(0.22)))
                            .child(action.unwrap_or("Check"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.stop_propagation();
                                Self::check_and_apply_update(cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The update item at the bottom of the sidebar, above the account.
    pub(super) fn render_version_footer(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        updates::ensure_release_check();
        let release = updates::release_status();
        let update = updates::current();
        let (label, action) = status_label(&release, &update, updates::is_source_build());
        let feedback = visible_feedback();
        let theme = Theme::global();
        let feedback = feedback.map(|(text, tone)| {
            let color = match tone {
                Tone::Busy => theme.ACCENT,
                Tone::Good => theme.OK,
                Tone::News => theme.ACCENT,
                Tone::Bad => theme.ERROR,
            };
            let content = if tone == Tone::Busy {
                let ticker = self.version_pulse.get(
                    "check",
                    crate::pulse_text::Kind::PulseText {
                        text: text.into(),
                        period: crate::pulse_text::TOOL_PULSE_PERIOD,
                        curve: version_check_pulse,
                    },
                    cx,
                );
                ticker.into_any_element()
            } else {
                div().child(text).into_any_element()
            };
            div()
                .id("workspace-version-check")
                .debug_selector(|| "workspace-version-check".into())
                .flex_none()
                .text_color(color)
                .font_family(theme.FONT_MONO)
                .text_size(px(9.0))
                .child(content)
        });
        if feedback.is_none() {
            self.version_pulse.retain(|_| false);
        }
        let detail = match &update {
            UpdateState::Finished { message } | UpdateState::Failed { message } => message.clone(),
            _ => match &release {
                ReleaseStatus::Error { message } => message.clone(),
                ReleaseStatus::Source => "Local source build. Release status cannot establish whether this checkout is current. Use Ctrl+R to rebuild local changes.".into(),
                ReleaseStatus::Current { latest } => format!("Latest release: v{latest}"),
                ReleaseStatus::Newer { version } if updates::is_source_build() => format!("v{version} is released. This source build updates through Git and Ctrl+R."),
                _ => label.clone(),
            },
        };
        div()
            .id("workspace-version")
            .debug_selector(|| "workspace-version".into())
            .flex_none()
            .mx_2()
            .mt_1()
            .h(px(22.0))
            .rounded_md()
            .bg(Theme::global().PANEL_BG)
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .occlude()
            .cursor_pointer()
            .hover(|el| el.bg(Theme::global().ACCENT.opacity(0.08)))
            .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .on_click(cx.listener(|_, _, _, cx| {
                cx.stop_propagation();
                Self::check_and_apply_update(cx);
                cx.notify();
            }))
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    live_tabs::TabTooltip(
                        format!(
                            "{}\nClick to check for updates",
                            if detail == label {
                                label.clone()
                            } else {
                                format!("{label}\n{detail}")
                            }
                        )
                        .into(),
                    )
                })
                .into()
            })
            .child(
                div()
                    .id("workspace-build")
                    .debug_selector(|| "workspace-build".into())
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .font_family(Theme::global().FONT_MONO)
                    .text_size(px(9.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .tooltip(|_, cx| cx.new(|_| crate::build_info::BuildTooltip).into())
                    .child(crate::build_info::version()),
            )
            .children(feedback)
            .when_some(action, |el, action| {
                el.child(
                    div()
                        .id("workspace-update-button")
                        .debug_selector(|| "workspace-update-button".into())
                        .flex_none()
                        .px_1()
                        .h(px(16.0))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(Theme::global().ACCENT.opacity(0.12))
                        .text_color(Theme::global().ACCENT)
                        .font_family(Theme::global().FONT_MONO)
                        .text_size(px(9.0))
                        .cursor_pointer()
                        .hover(|el| el.bg(Theme::global().ACCENT.opacity(0.22)))
                        .child(action),
                )
            })
            .into_any_element()
    }
}

fn version_check_pulse(phase: f32) -> f32 {
    0.6 + 0.4 * (phase * std::f32::consts::TAU).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_status_never_calls_an_unchecked_build_latest() {
        for release in [
            ReleaseStatus::Unknown,
            ReleaseStatus::Checking,
            ReleaseStatus::Source,
            ReleaseStatus::Error {
                message: "offline".into(),
            },
        ] {
            let (label, action) = status_label(&release, &UpdateState::Idle, false);
            assert_ne!(label, "Latest version");
            assert!(action.is_none());
        }
        assert_eq!(
            status_label(
                &ReleaseStatus::Current {
                    latest: "1.0.0".into()
                },
                &UpdateState::Idle,
                false
            ),
            ("Latest version".into(), None)
        );
        let newer = ReleaseStatus::Newer {
            version: "1.1.0".into(),
        };
        assert_eq!(
            status_label(&newer, &UpdateState::Idle, false).1,
            Some("Update")
        );
        assert_eq!(
            status_label(&newer, &UpdateState::Idle, true).1,
            None,
            "a source checkout cannot install a release"
        );
        assert_eq!(status_label(&newer, &UpdateState::Checking, false).1, None);
        assert_eq!(
            status_label(
                &newer,
                &UpdateState::ReadyToRestart {
                    version: "1.1.0".into()
                },
                false
            )
            .1,
            Some("Restart")
        );
    }

    #[test]
    fn a_requested_check_reports_progress_then_its_result() {
        let current = ReleaseStatus::Current {
            latest: "1.0.0".into(),
        };
        assert_eq!(
            check_feedback(&ReleaseStatus::Checking, &UpdateState::Idle),
            Some(("Checking…".into(), Tone::Busy))
        );
        assert_eq!(
            check_feedback(&current, &UpdateState::Checking),
            Some(("Checking…".into(), Tone::Busy))
        );
        assert_eq!(
            check_feedback(&current, &UpdateState::Idle),
            Some(("Up to date".into(), Tone::Good))
        );
        assert_eq!(
            check_feedback(
                &current,
                &UpdateState::Finished {
                    message: "current".into()
                }
            ),
            Some(("Up to date".into(), Tone::Good))
        );
        assert_eq!(
            check_feedback(
                &ReleaseStatus::Newer {
                    version: "1.1.0".into()
                },
                &UpdateState::Idle
            ),
            Some(("v1.1.0 out".into(), Tone::News))
        );
        assert_eq!(
            check_feedback(
                &ReleaseStatus::Error {
                    message: "offline".into()
                },
                &UpdateState::Idle
            )
            .map(|(_, tone)| tone),
            Some(Tone::Bad)
        );
        assert_eq!(
            check_feedback(
                &current,
                &UpdateState::ReadyToRestart {
                    version: "1.1.0".into()
                }
            ),
            None,
            "Restart already says it"
        );
    }
    #[gpui::test]
    fn update_footer_shows_only_when_actionable_and_version_lives_in_settings(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static REQUESTS: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn check() {
            REQUESTS.fetch_add(1, Ordering::SeqCst);
        }
        let _guard = updates::test_lock();
        updates::clear_test_actions();
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Current {
            latest: "1.0.0".into(),
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.push_test_panel("version-header", cx);
            w
        });
        let draw = |vcx: &mut gpui::VisualTestContext| {
            workspace.update(vcx, |_, cx| cx.notify());
            vcx.draw(
                gpui::point(px(0.), px(0.)),
                gpui::size(px(1200.), px(800.)),
                |_, _| div(),
            );
            vcx.run_until_parked();
        };
        draw(vcx);
        // Nothing to do: no strip, and the canvas keeps the full height.
        assert!(vcx.debug_bounds("workspace-version").is_none());
        assert!(!footer_visible());
        assert_eq!(footer_height(), 0.0);
        assert!(vcx.debug_bounds("panel-build").is_none());

        // A newer release offers Update in the footer.
        updates::set_release_status(ReleaseStatus::Newer {
            version: "1.1.0".into(),
        });
        unsafe {
            updates::jcode_update_register_actions(check, check);
        }
        draw(vcx);
        assert_eq!(footer_height(), 0.0, "the canvas never reserves the strip");
        let header = vcx.debug_bounds("workspace-version").unwrap();
        let button = vcx
            .debug_bounds("workspace-update-button")
            .expect("new release offers Update");
        let account = vcx.debug_bounds("sidebar-account").expect("sidebar shown");
        assert!(header.right() <= account.right() + px(8.), "lives in the sidebar");
        assert!(header.bottom() <= account.origin.y, "sits above the account");
        assert!(button.right() <= header.right());
        let before = REQUESTS.load(Ordering::SeqCst);
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        draw(vcx);
        assert_eq!(REQUESTS.load(Ordering::SeqCst), before + 1);
        assert!(
            vcx.debug_bounds("workspace-update-button").is_none(),
            "busy update cannot be clicked twice"
        );

        // A failed update keeps Retry.
        updates::set(UpdateState::Failed {
            message: "boom".into(),
        });
        draw(vcx);
        assert!(vcx.debug_bounds("workspace-update-button").is_some());

        // Settings → About shows the version and a manual check.
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Current {
            latest: "1.0.0".into(),
        });
        workspace.update(vcx, |w, cx| {
            w.show_sidebar = true;
            w.sidebar_view = SidebarView::Settings;
            cx.notify();
        });
        draw(vcx);
        assert!(vcx.debug_bounds("workspace-version").is_none());
        assert!(vcx.debug_bounds("settings-about").is_some());
        assert!(vcx.debug_bounds("settings-about-version").is_some());
        let check_button = vcx.debug_bounds("settings-check-updates").unwrap();
        let before = REQUESTS.load(Ordering::SeqCst);
        vcx.simulate_click(check_button.center(), gpui::Modifiers::default());
        draw(vcx);
        assert_eq!(REQUESTS.load(Ordering::SeqCst), before + 1);
        updates::clear_test_actions();
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Source);
    }

    /// A fresh packaged launch has no release state yet, so `footer_visible()`
    /// is false and the footer never renders. If the check starts only from
    /// renders that need it, a user who stays in the session view never learns
    /// about a release and never gets an automatic update.
    #[gpui::test]
    fn session_view_starts_the_release_check_before_any_footer_shows(
        cx: &mut gpui::TestAppContext,
    ) {
        let _guard = updates::test_lock();
        updates::clear_test_actions();
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Unknown);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.push_test_panel("release-at-launch", cx);
            w
        });
        assert!(
            !footer_visible(),
            "an unstarted check offers nothing to do, so the footer stays hidden"
        );
        workspace.update(vcx, |_, cx| cx.notify());
        vcx.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(1200.), px(800.)),
            |_, _| div(),
        );
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("workspace-version").is_none(),
            "the hidden footer paints nothing"
        );
        assert_ne!(
            updates::release_status(),
            ReleaseStatus::Unknown,
            "the session view starts the release check without the footer"
        );
        updates::set_release_status(ReleaseStatus::Source);
    }
}
