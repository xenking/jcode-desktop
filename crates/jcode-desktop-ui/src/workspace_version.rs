//! Where the Desktop build version is shown, and the app's single update
//! control.
//!
//! The version lives in exactly two places: Settings → About and the account
//! popup. The panel footer never carries build text. Update indication is the
//! sidebar account avatar: while an update is worth acting on it wears a badge,
//! and the account popup names the installed and the offered version with the
//! one button that applies it. Nothing else in the chrome mentions versions.
use super::*;
use crate::updates::{ReleaseStatus, UpdateState};

/// What the sidebar offers about an update, when it offers anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UpdateOffer {
    /// The popup button: "Update", "Retry" or "Restart".
    pub action: &'static str,
    /// The line the badge tooltip and the popup show.
    pub label: String,
}

/// Write a release version the way the user reads it, without doubling a `v`
/// the updater may already include.
fn version_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_owned()
    } else {
        format!("v{version}")
    }
}

/// An offer exists only while a click would do something: install a newer
/// release, continue a download, retry a failure, or restart into a staged
/// build. A current, unchecked, or checking build offers nothing, and a source
/// checkout cannot install a release in place, so it offers nothing either.
pub(super) fn update_offer(
    release: &ReleaseStatus,
    update: &UpdateState,
    source_build: bool,
) -> Option<UpdateOffer> {
    let offer = |action, label: String| Some(UpdateOffer { action, label });
    match update {
        // The popup's version lines already say how a check is going, and a
        // finished Linux request has no follow-up click.
        UpdateState::Checking | UpdateState::Finished { .. } => None,
        UpdateState::Available { version } => {
            offer("Update", format!("{} downloading", version_tag(version)))
        }
        UpdateState::ReadyToRestart { version } => offer(
            "Restart",
            format!("{} ready to restart", version_tag(version)),
        ),
        UpdateState::Failed { .. } => offer("Retry", "Update failed".into()),
        UpdateState::Idle => match release {
            ReleaseStatus::Newer { version } if !source_build => {
                offer("Update", format!("{} available", version_tag(version)))
            }
            // Source builds, up-to-date builds, unchecked builds and release
            // errors all have nothing to install.
            _ => None,
        },
    }
}

/// The avatar's badge, read straight from the updater for the render path.
pub(super) fn offered_update() -> Option<UpdateOffer> {
    update_offer(
        &updates::release_status(),
        &updates::current(),
        updates::is_source_build(),
    )
}

/// The two version lines the account popup shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VersionBlock {
    /// The build running now, e.g. `v0.7.0 · Dev 16`.
    pub installed: String,
    /// The release a click would install, when one is offered.
    pub offer: Option<UpdateOffer>,
}

pub(super) fn version_block(
    release: &ReleaseStatus,
    update: &UpdateState,
    source_build: bool,
) -> VersionBlock {
    VersionBlock {
        installed: crate::build_info::display_version(),
        offer: update_offer(release, update, source_build),
    }
}

/// The popup's version block: the installed build, then the release on offer
/// and the button that applies it. Rendered from [`VersionBlock`] so the text
/// and the button are testable without a signed-in account.
pub(super) fn render_version_block(
    block: &VersionBlock,
    on_action: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let theme = Theme::global();
    let mut section = div()
        .debug_selector(|| "account-menu-version-block".into())
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .debug_selector(|| "account-menu-version".into())
                        .font_family(theme.FONT_MONO)
                        .text_color(theme.TEXT_DIM)
                        .child(block.installed.clone()),
                )
                .children(block.offer.as_ref().map(|offer| {
                    div()
                        .debug_selector(|| "account-menu-update".into())
                        .min_w_0()
                        .truncate()
                        .text_color(theme.ACCENT)
                        .child(offer.label.clone())
                })),
        );
    if let Some(offer) = &block.offer {
        section = section.child(
            div()
                .id("account-update-button")
                .debug_selector(|| "account-update-button".into())
                .h(px(26.0))
                .px_3()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .bg(theme.ACCENT)
                .text_color(theme.BG)
                .hover(|el| el.opacity(0.85))
                .child(offer.action)
                .on_click(on_action),
        );
    }
    section.into_any_element()
}

impl Workspace {
    /// The one place a user-requested update runs: the sidebar badge, the
    /// account popup button, Settings → About and the native app menu all
    /// call it. Ask the platform updater to check, download, or install
    /// whatever is next.
    pub(crate) fn check_and_apply_update(cx: &mut gpui::App) {
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
        let block = version_block(&release, &update, updates::is_source_build());
        let status = about_status(&update, &release, block.offer.as_ref());
        render_about_block(
            &block,
            &status,
            cx.listener(|_, _, _, cx| {
                cx.stop_propagation();
                Self::check_and_apply_update(cx);
                cx.notify();
            }),
        )
    }
}

/// What Settings → About says beside the version. It names the released
/// version, or the state of the check, but never repeats the installed number.
fn about_status(
    update: &UpdateState,
    release: &ReleaseStatus,
    offer: Option<&UpdateOffer>,
) -> String {
    match (update, release) {
        (UpdateState::Failed { .. }, _) => "Update failed".to_string(),
        (UpdateState::ReadyToRestart { .. }, _) => "Restart to finish".to_string(),
        (UpdateState::Available { .. }, _) => "Downloading…".to_string(),
        (UpdateState::Checking, _) => "Checking for updates…".to_string(),
        (_, ReleaseStatus::Current { latest }) => {
            format!("No channel update ({})", version_tag(latest))
        }
        (_, ReleaseStatus::Source) => "Local source build".to_string(),
        (_, ReleaseStatus::Error { .. }) => "Unable to check".to_string(),
        (_, ReleaseStatus::Checking) | (_, ReleaseStatus::Unknown) => {
            "Checking for updates…".to_string()
        }
        (_, ReleaseStatus::Newer { .. }) => {
            offer.map_or_else(|| "Latest version".to_string(), |offer| offer.label.clone())
        }
    }
}

/// The About row itself. Split from [`Workspace::render_about_settings`] so the
/// version, its status and the check button are testable without scrolling the
/// settings list.
pub(super) fn render_about_block(
    block: &VersionBlock,
    status: &str,
    on_check: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let theme = Theme::global();
    let action = block.offer.as_ref().map(|offer| offer.action);
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
                        .child(block.installed.clone()),
                )
                .child(
                    div()
                        .id("settings-about-status")
                        .debug_selector(|| "settings-about-status".into())
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.TEXT_DIM)
                        .tooltip(|_, cx| {
                            cx.new(|_| live_tabs::TabTooltip(
                                "Checks this update channel. Betas published separately may not appear here."
                                    .into(),
                            ))
                            .into()
                        })
                        .child(status.to_string()),
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
                        .on_click(on_check),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_actionable_update_offers_anything() {
        let newer = ReleaseStatus::Newer {
            version: "1.1.0".into(),
        };
        for quiet in [
            ReleaseStatus::Unknown,
            ReleaseStatus::Checking,
            ReleaseStatus::Source,
            ReleaseStatus::Current {
                latest: "1.0.0".into(),
            },
            ReleaseStatus::Error {
                message: "offline".into(),
            },
        ] {
            assert_eq!(
                update_offer(&quiet, &UpdateState::Idle, false),
                None,
                "{quiet:?} has nothing to install"
            );
        }
        assert_eq!(
            update_offer(&newer, &UpdateState::Idle, false),
            Some(UpdateOffer {
                action: "Update",
                label: "v1.1.0 available".into(),
            })
        );
        assert_eq!(
            update_offer(&newer, &UpdateState::Idle, true),
            None,
            "a source checkout cannot install a release"
        );
        assert_eq!(
            update_offer(
                &newer,
                &UpdateState::Available {
                    version: "1.1.0".into()
                },
                false
            ),
            Some(UpdateOffer {
                action: "Update",
                label: "v1.1.0 downloading".into(),
            })
        );
        assert_eq!(
            update_offer(
                &newer,
                &UpdateState::ReadyToRestart {
                    version: "1.1.0".into()
                },
                false
            ),
            Some(UpdateOffer {
                action: "Restart",
                label: "v1.1.0 ready to restart".into(),
            })
        );
        assert_eq!(
            update_offer(&newer, &UpdateState::Checking, false),
            None,
            "a check in flight has no button"
        );
        assert_eq!(
            update_offer(
                &newer,
                &UpdateState::Failed {
                    message: "boom".into()
                },
                false
            )
            .map(|offer| offer.action),
            Some("Retry")
        );
        assert_eq!(
            update_offer(
                &newer,
                &UpdateState::Finished {
                    message: "done".into()
                },
                false
            ),
            None
        );
        // Sparkle reports bare versions; the label never doubles the prefix.
        assert_eq!(version_tag("1.1.0"), "v1.1.0");
        assert_eq!(version_tag("v1.1.0"), "v1.1.0");
    }

    #[test]
    fn the_popup_block_names_the_installed_and_the_offered_version() {
        let newest = version_block(
            &ReleaseStatus::Current {
                latest: "1.0.0".into(),
            },
            &UpdateState::Idle,
            false,
        );
        assert_eq!(newest.installed, crate::build_info::display_version());
        assert!(newest.installed.starts_with('v'));
        assert_eq!(newest.offer, None, "a current build offers nothing");

        let offered = version_block(
            &ReleaseStatus::Newer {
                version: "0.2.0-beta.1".into(),
            },
            &UpdateState::Idle,
            false,
        );
        assert_eq!(offered.installed, crate::build_info::display_version());
        let offer = offered.offer.expect("a newer release is offered");
        assert_eq!(offer.action, "Update");
        assert_eq!(offer.label, "v0.2.0-beta.1 available");
    }

    /// The version block is the account popup's content: both lines paint, the
    /// button only while an offer exists, and clicking it runs the update.
    #[gpui::test]
    fn the_popup_block_paints_versions_and_only_offers_a_button_when_an_update_exists(
        cx: &mut gpui::TestAppContext,
    ) {
        struct Probe {
            block: VersionBlock,
            clicks: std::rc::Rc<std::cell::Cell<usize>>,
        }

        impl gpui::Render for Probe {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                let clicks = self.clicks.clone();
                render_version_block(&self.block, move |_, _, _| {
                    clicks.set(clicks.get() + 1);
                })
            }
        }

        let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
        let quiet = version_block(
            &ReleaseStatus::Current {
                latest: "1.0.0".into(),
            },
            &UpdateState::Idle,
            false,
        );
        let (_probe, vcx) = cx.add_window_view(|_, _| Probe {
            block: quiet,
            clicks: clicks.clone(),
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("account-menu-version").is_some());
        assert!(
            vcx.debug_bounds("account-menu-update").is_none(),
            "no offered version, so no update line"
        );
        assert!(
            vcx.debug_bounds("account-update-button").is_none(),
            "no update means no button"
        );

        let offered = version_block(
            &ReleaseStatus::Newer {
                version: "0.2.0-beta.1".into(),
            },
            &UpdateState::Idle,
            false,
        );
        let (_probe, vcx) = cx.add_window_view(|_, _| Probe {
            block: offered,
            clicks: clicks.clone(),
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("account-menu-version").is_some());
        assert!(vcx.debug_bounds("account-menu-update").is_some());
        let button = vcx
            .debug_bounds("account-update-button")
            .expect("a newer release offers Update");
        let version = vcx.debug_bounds("account-menu-version").unwrap();
        assert!(
            button.top() >= version.bottom(),
            "the button follows the versions"
        );
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(clicks.get(), 1, "the button runs the update");
    }

    /// The whole workspace chrome: no version in the footer, and Settings →
    /// About is where the installed build is named and can be checked.
    #[gpui::test]
    fn the_footer_carries_no_version_and_settings_about_names_it(cx: &mut gpui::TestAppContext) {
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
        for selector in [
            "workspace-version",
            "workspace-build",
            "workspace-update-button",
            "panel-build",
        ] {
            assert!(
                vcx.debug_bounds(selector).is_none(),
                "{selector} must not paint anywhere in the chrome"
            );
        }

        // Settings → About is where the build is named.
        workspace.update(vcx, |w, cx| {
            w.show_sidebar = true;
            w.sidebar_view = SidebarView::Settings;
            cx.notify();
        });
        draw(vcx);
        assert!(vcx.debug_bounds("settings-about").is_some());
        assert!(vcx.debug_bounds("settings-about-version").is_some());
        assert!(vcx.debug_bounds("workspace-version").is_none());
        updates::clear_test_actions();
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Source);
    }

    /// Settings → About is the second and last place the version appears. Its
    /// row names the build, describes the release state, and its button runs a
    /// real check.
    #[gpui::test]
    fn the_about_row_names_the_version_and_its_button_checks(cx: &mut gpui::TestAppContext) {
        struct Probe {
            block: VersionBlock,
            status: String,
            checks: std::rc::Rc<std::cell::Cell<usize>>,
        }

        impl gpui::Render for Probe {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                let checks = self.checks.clone();
                render_about_block(&self.block, &self.status, move |_, _, _| {
                    checks.set(checks.get() + 1);
                })
            }
        }

        let checks = std::rc::Rc::new(std::cell::Cell::new(0));
        let offer = UpdateOffer {
            action: "Update",
            label: "v0.2.0-beta.1 available".into(),
        };
        let block = VersionBlock {
            installed: crate::build_info::display_version(),
            offer: Some(offer.clone()),
        };
        let status = about_status(
            &UpdateState::Idle,
            &ReleaseStatus::Newer {
                version: "0.2.0-beta.1".into(),
            },
            Some(&offer),
        );
        let (_probe, vcx) = cx.add_window_view(|_, _| Probe {
            block,
            status,
            checks: checks.clone(),
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("settings-about").is_some());
        assert!(vcx.debug_bounds("settings-about-version").is_some());
        assert!(vcx.debug_bounds("settings-about-status").is_some());
        let button = vcx
            .debug_bounds("settings-check-updates")
            .expect("About offers a check");
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(checks.get(), 1, "the About button runs the update path");
    }

    /// The status line never claims a release is current when nothing checked it.
    #[test]
    fn the_about_status_never_calls_an_unchecked_build_latest() {
        for (update, release) in [
            (UpdateState::Idle, ReleaseStatus::Unknown),
            (UpdateState::Idle, ReleaseStatus::Checking),
            (UpdateState::Idle, ReleaseStatus::Source),
            (
                UpdateState::Idle,
                ReleaseStatus::Error {
                    message: "offline".into(),
                },
            ),
        ] {
            let status = about_status(&update, &release, None);
            assert!(
                !status.contains("Latest"),
                "{release:?} must not read as current: {status}"
            );
        }
        assert_eq!(
            about_status(
                &UpdateState::Idle,
                &ReleaseStatus::Current {
                    latest: "1.0.0".into()
                },
                None
            ),
            "No channel update (v1.0.0)"
        );
        assert_eq!(
            about_status(
                &UpdateState::ReadyToRestart {
                    version: "1.1.0".into()
                },
                &ReleaseStatus::Newer {
                    version: "1.1.0".into()
                },
                None
            ),
            "Restart to finish"
        );
    }

    /// The avatar is the app's only update control: a plain avatar while the
    /// build is current, a badge only while an update is worth acting on, and
    /// the press updates instead of opening the account popup.
    #[gpui::test]
    fn the_sidebar_avatar_wears_a_badge_only_while_an_update_exists(cx: &mut gpui::TestAppContext) {
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
            w.push_test_panel("badge-header", cx);
            w.show_sidebar = true;
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
        assert!(
            vcx.debug_bounds("sidebar-account-avatar").is_some(),
            "the plain avatar always paints"
        );
        assert!(
            vcx.debug_bounds("sidebar-update-badge").is_none(),
            "a current build shows no update badge"
        );

        // A newer release turns the avatar into the update control.
        updates::set_release_status(ReleaseStatus::Newer {
            version: "1.1.0".into(),
        });
        unsafe {
            updates::jcode_update_register_actions(check, check);
        }
        draw(vcx);
        let badge = vcx
            .debug_bounds("sidebar-update-badge")
            .expect("a newer release badges the avatar");
        let avatar = vcx.debug_bounds("sidebar-account-avatar").unwrap();
        assert!(
            badge.size.width > px(0.) && badge.size.height > px(0.),
            "the badge occupies real space: {badge:?}"
        );
        assert!(
            badge.origin.x >= avatar.origin.x - px(4.) && badge.right() <= avatar.right() + px(4.),
            "the badge sits on the avatar: {badge:?} {avatar:?}"
        );
        assert!(
            vcx.debug_bounds("workspace-version").is_none(),
            "the old sidebar strip is gone, not duplicated"
        );

        let before = REQUESTS.load(Ordering::SeqCst);
        vcx.simulate_click(badge.center(), gpui::Modifiers::default());
        draw(vcx);
        assert_eq!(
            REQUESTS.load(Ordering::SeqCst),
            before + 1,
            "pressing the badge runs the update"
        );
        assert!(
            !workspace.read_with(vcx, |w, _| w.account_menu.open),
            "the badge press must not open the account popup"
        );

        // A staged build keeps the badge so the restart stays reachable.
        updates::set(UpdateState::ReadyToRestart {
            version: "1.1.0".into(),
        });
        draw(vcx);
        assert!(vcx.debug_bounds("sidebar-update-badge").is_some());

        // Back to current: the badge goes away and the avatar is plain again.
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Current {
            latest: "1.1.0".into(),
        });
        draw(vcx);
        assert!(vcx.debug_bounds("sidebar-update-badge").is_none());
        assert!(vcx.debug_bounds("sidebar-account-avatar").is_some());
        updates::clear_test_actions();
        updates::set(UpdateState::Idle);
        updates::set_release_status(ReleaseStatus::Source);
    }

    /// A fresh packaged launch has no release state yet, so the avatar wears no
    /// badge and no update surface is on screen. If the check started only from
    /// renders that need it, a user who stays in the session view would never
    /// learn about a release and never get an automatic update.
    #[gpui::test]
    fn session_view_starts_the_release_check_before_any_update_surface_shows(
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
        assert_eq!(
            update_offer(
                &updates::release_status(),
                &updates::current(),
                updates::is_source_build()
            ),
            None,
            "an unstarted check offers nothing, so the avatar wears no badge"
        );
        workspace.update(vcx, |_, cx| cx.notify());
        vcx.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(1200.), px(800.)),
            |_, _| div(),
        );
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("sidebar-update-badge").is_none(),
            "no update surface paints anything yet"
        );
        assert!(
            vcx.debug_bounds("workspace-version").is_none(),
            "the version strip does not exist any more"
        );
        assert_ne!(
            updates::release_status(),
            ReleaseStatus::Unknown,
            "the session view starts the release check without any update surface"
        );
        updates::set_release_status(ReleaseStatus::Source);
    }
}
