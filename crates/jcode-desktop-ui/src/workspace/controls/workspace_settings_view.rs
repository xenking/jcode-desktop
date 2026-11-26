//! The accounts and settings pages.
use super::*;

impl Workspace {
    /// The dedicated tab fills the sidebar, with fixed-height account rows and
    /// independent scrolling when the credentials exceed the available height.
    pub(super) fn render_accounts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.accounts.is_empty() && !self.has_routing_rows() {
            return None;
        }

        let section = div()
            .debug_selector(|| "accounts-section".into())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .py_1()
            .child(
                div()
                    .flex_none()
                    .px_4()
                    .pb(px(2.0))
                    .text_size(px(10.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child("accounts"),
            );

        let active = self.slots.get(self.active).and_then(|slot| {
            let panel = slot.panel.read_chat(cx)?;
            panel
                .provider
                .as_deref()
                .map(|provider| accounts::credential_id(provider, panel.auth_method.as_deref()))
        });
        let mut list = div()
            .id("accounts-list")
            .debug_selector(|| "accounts-list".into())
            .h_full()
            .overflow_hidden()
            .track_scroll(&self.accounts_scroll)
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let max = f32::from(this.accounts_scroll.max_offset().y).max(0.0);
                    let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
                    this.accounts_scroll_remainder += dy;
                    let rows = (this.accounts_scroll_remainder / ACCOUNT_ROW_HEIGHT).trunc();
                    if rows != 0.0 {
                        this.accounts_scroll_remainder -= rows * ACCOUNT_ROW_HEIGHT;
                        let old = f32::from(this.accounts_scroll.offset().y);
                        let next = (old + rows * ACCOUNT_ROW_HEIGHT).clamp(-max, 0.0);
                        this.accounts_scroll
                            .set_offset(gpui::point(px(0.0), px(next)));
                        cx.notify();
                    }
                    // Keep the compact rows' scrolling cadence, including taller history cards.
                    cx.stop_propagation();
                }),
            )
            .flex()
            .flex_col();
        let mut previous_availability = None;
        for (index, account) in
            accounts::ordered(&self.accounts, active.as_deref(), &self.recent_accounts)
                .into_iter()
                .enumerate()
        {
            let available = account.available();
            if previous_availability != Some(available) {
                list = list.child(
                    div()
                        .debug_selector(move || {
                            if available {
                                "accounts-connected-heading".into()
                            } else {
                                "accounts-disconnected-heading".into()
                            }
                        })
                        .flex_none()
                        .px_4()
                        .pt_2()
                        .pb_1()
                        .text_size(px(10.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(if available {
                            "Connected"
                        } else {
                            "Not connected"
                        }),
                );
                previous_availability = Some(available);
            }
            let ink = if available {
                Theme::global().TEXT
            } else {
                Theme::global().TEXT_DIM
            };

            let logo: gpui::AnyElement = match accounts::logo(&account.id) {
                Some(bytes) => gpui::svg()
                    .data(bytes)
                    .size(px(16.0))
                    .flex_none()
                    .text_color(ink)
                    .into_any_element(),
                None => div()
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .bg(Theme::global().INLINE_CODE_BG)
                    .text_size(px(10.0))
                    .text_color(ink)
                    .child(accounts::lettermark(&account.display_name))
                    .into_any_element(),
            };

            let mut details = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(16.0))
                        .line_height(px(16.0))
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(ink)
                                .child(account.display_name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(9.0))
                                .text_color(Theme::global().TEXT_DIM)
                                .child(account.auth_kind.clone()),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| format!("account-{}-status", account.id))
                        .text_size(px(9.0))
                        .line_height(px(13.0))
                        .text_color(if account.status == "expired" {
                            Theme::global().WARN
                        } else {
                            Theme::global().TEXT_DIM
                        })
                        .child(account.status_label()),
                );

            // One compact pill per login with a redeemable reset. The pill only
            // opens a review. Nothing is spent until it is confirmed.
            let make_pill = |reset_index: usize,
                                 reset: accounts::BankedReset,
                                 label: String|
             -> gpui::AnyElement {
                div()
                                .id(("account-reset", index * 100 + reset_index))
                                .debug_selector({
                                    let id = account.id.clone();
                                    move || format!("account-{id}-reset-{reset_index}")
                                })
                                .flex_none()
                                .max_w(px(148.0))
                                .truncate()
                                .px(px(8.0))
                                .h(px(16.0))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .cursor_pointer()
                                .text_size(px(9.0))
                                .bg(Theme::global().wash(Ink::Accent, 0.16))
                                .text_color(Theme::global().ACCENT)
                                .hover(|el| el.bg(Theme::global().wash(Ink::Accent, 0.26)))
                                .tooltip(crate::components::Tooltip::text("Review a banked usage reset. Nothing is spent until you confirm.").builder())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.open_usage_reset(&reset, window, cx);
                                }))
                                .child(label)
                    .into_any_element()
            };
            // Every saved login gets its own block: name, plan, windows, resets
            // and credits. A provider without saved-login identities keeps the
            // compact single row below.
            let mut offered = 0;
            let login_blocks = login_rows::login_blocks(
                account,
                &|label| self.account_icon(&account.id, label),
                &mut |reset| {
                    if !reset.offerable() {
                        return None;
                    }
                    let reset_index = offered;
                    offered += 1;
                    Some(make_pill(reset_index, reset.clone(), reset.pill_label()))
                },
            )
            .filter(|_| available);
            let lists_logins = login_blocks.is_some();
            if let Some(blocks) = login_blocks {
                details = details.child(blocks);
            } else if available && !account.limits.is_empty() {
                // A single two-column quota row retains the useful summary
                // without letting one account grow into a card.
                let limit_count = if account.id == "antigravity" {
                    1
                } else {
                    account.limits.len().min(2)
                };
                let visible_limits = &account.limits[..limit_count];
                let mut limits = div().mt(px(3.0)).flex().flex_col().gap(px(4.0));
                for (row_index, row) in visible_limits.chunks(2).enumerate() {
                    let mut limit_row = div().flex().gap(px(6.0));
                    for (column_index, limit) in row.iter().enumerate() {
                        let limit_index = row_index * 2 + column_index;
                        let used = limit.usage_percent.clamp(0.0, 100.0);
                        let label = match &limit.reset_in {
                            Some(reset) => format!("{} · {:.0}% · {reset}", limit.name, used),
                            None => format!("{} · {:.0}%", limit.name, used),
                        };
                        limit_row = limit_row.child(
                            div()
                                .id(("account-limit", index * 1000 + limit_index))
                                .debug_selector({
                                    let id = account.id.clone();
                                    move || format!("account-{id}-limit-{limit_index}")
                                })
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .h(px(12.0))
                                        .line_height(px(12.0))
                                        .text_size(px(8.0))
                                        .text_color(Theme::global().TEXT_DIM)
                                        .child(label),
                                )
                                .child(usage_bar(
                                    used / 100.0,
                                    if used >= 90.0 {
                                        Theme::global().ERROR
                                    } else if used >= 70.0 {
                                        Theme::global().WARN
                                    } else {
                                        Theme::global().ACCENT
                                    },
                                )),
                        );
                    }
                    limits = limits.child(limit_row);
                }
                details = details.child(limits);
            }
            if !lists_logins
                && available
                && let Some(credits) = login_rows::provider_credits(account)
            {
                details = details.child(credits);
            }

            if available {
                // One compact pill per login with a redeemable reset. The pill
                // only opens a review. Nothing is spent until it is confirmed.
                // A listed login carries its own pill inside its block.
                let resets: Vec<accounts::BankedReset> = if lists_logins {
                    Vec::new()
                } else {
                    account.offerable_resets().cloned().collect()
                };
                let many = resets.len() > 1;
                let has_resets = !resets.is_empty();
                let mut pills = div().mt(px(3.0)).flex().flex_wrap().gap(px(4.0));
                for (reset_index, reset) in resets.into_iter().enumerate() {
                    // Name the login the reset belongs to. A listed login has a
                    // name people know, never just its internal label.
                    let owner = if lists_logins {
                        account
                            .usage_reports
                            .iter()
                            .find(|report| report.banked_reset.as_ref() == Some(&reset))
                            .and_then(|report| report.identity.as_ref())
                            .map(|identity| (None, identity.display_name.clone()))
                    } else {
                        reset.account_label.as_deref().filter(|_| many).map(|label| {
                            (Some(self.account_icon(&account.id, label)), label.to_owned())
                        })
                    };
                    let label = match owner {
                        // The login's icon leads the whole pill.
                        Some((Some(icon), login)) => {
                            format!("{icon} {} · {login}", reset.pill_label())
                        }
                        Some((None, login)) => format!("{} · {login}", reset.pill_label()),
                        None => reset.pill_label(),
                    };
                    pills = pills.child(make_pill(reset_index, reset, label));
                }
                if has_resets {
                    details = details.child(pills);
                }
                if let Some((label, url)) = account.upgrade_offer() {
                    details = details.child(
                        div().mt(px(3.0)).flex().child(
                            div()
                                .id(("account-upgrade", index))
                                .debug_selector({
                                    let id = account.id.clone();
                                    move || format!("account-{id}-upgrade")
                                })
                                .flex_none()
                                .px(px(8.0))
                                .h(px(16.0))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .cursor_pointer()
                                .text_size(px(9.0))
                                .bg(Theme::global().wash(Ink::Warn, 0.16))
                                .text_color(Theme::global().WARN)
                                .hover(|el| el.bg(Theme::global().wash(Ink::Warn, 0.26)))
                                .tooltip(crate::components::Tooltip::text("Opens Jcode pricing in your browser. Nothing is purchased here.").builder())
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    cx.open_url(&url);
                                })
                                .child(label),
                        ),
                    );
                }
            }

            if account.shows_oauth_history() {
                // One short line per login; estimated amounts are detailed on hover.
                let mut history = div()
                    .debug_selector(|| format!("account-{}-history", account.id))
                    .mt(px(3.0))
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .text_size(px(9.0))
                    .line_height(px(13.0))
                    .text_color(Theme::global().TEXT_DIM);
                if account.usage_reports.is_empty() {
                    history = history.child(
                        div()
                            .debug_selector(|| "account-openai-history-unavailable".into())
                            .child("No usage recorded yet"),
                    );
                }
                let many = account.usage_reports.len() > 1;
                for (report_index, report) in account.usage_reports.iter().enumerate() {
                    let amount =
                        |period| report.period_amount(period).unwrap_or_else(|| "n/a".into());
                    let icon = report
                        .account_label
                        .as_deref()
                        .map(|label| self.account_icon(&account.id, label));
                    let tooltip = crate::panel::meter::RowsTooltip {
                        title: format!(
                            "{}{} · API estimate, not a bill",
                            icon.as_deref().map(|icon| format!("{icon} ")).unwrap_or_default(),
                            report.title()
                        ),
                        rows: ["Today", "Lifetime"]
                            .into_iter()
                            .map(|period| format!("{period}: {}", amount(period)))
                            .collect(),
                    };
                    let summary =
                        format!("today {} · total {}", amount("Today"), amount("Lifetime"));
                    let label = report.account_label.clone().filter(|_| many);
                    history = history.child(
                        div()
                            .id(("account-history", index * 100 + report_index))
                            .debug_selector(move || {
                                format!("account-openai-history-report-{report_index}")
                            })
                            .flex()
                            .gap_1()
                            .min_w_0()
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::panel::meter::RowsTooltip {
                                    title: tooltip.title.clone(),
                                    rows: tooltip.rows.clone(),
                                })
                                .into()
                            })
                            .when_some(label, |el, label| {
                                // The login's icon leads its line.
                                el.children(icon.clone().map(|icon| {
                                    div()
                                        .debug_selector(move || {
                                            format!("account-openai-history-report-{report_index}-icon")
                                        })
                                        .flex_none()
                                        .child(icon)
                                }))
                                .child(
                                    div()
                                        .flex_shrink(1.0)
                                        .min_w(px(24.0))
                                        .truncate()
                                        .text_color(ink)
                                        .child(label),
                                )
                            })
                            .child(div().flex_1().min_w_0().truncate().child(summary)),
                    );
                }
                details = details.child(history);
            }

            // Rows with history or a reset pill need more than the compact height.
            let grows = account.shows_oauth_history()
                || lists_logins
                || (available && account.offerable_resets().next().is_some())
                || (available && account.upgrade_offer().is_some());
            list = list.child(
                div()
                    .flex_none()
                    .id(("account", index))
                    .debug_selector(|| format!("account-{}", account.id))
                    .mx_2()
                    .px_2()
                    .when(grows, |row| row.min_h(px(ACCOUNT_ROW_HEIGHT)))
                    .when(!grows, |row| row.h(px(ACCOUNT_ROW_HEIGHT)))
                    .py_1()
                    .overflow_hidden()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .when(grows, |row| row.items_start())
                    .gap_2()
                    .hover(|el| el.bg(Theme::global().HEADER_BG))
                    .child(logo)
                    .child(details)
                    .child(
                        div()
                            .flex_none()
                            .size(px(5.0))
                            .rounded_full()
                            .bg(if available {
                                Theme::global().OK
                            } else if account.status == "expired" {
                                Theme::global().WARN
                            } else {
                                Theme::global().TEXT_FAINT
                            }),
                    ),
            );
        }

        let list = self.append_routing_rows(list);

        Some(
            section
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .pr(px(crate::scrollbar::GUTTER))
                        .child(list)
                        .child(crate::scrollbar::vertical_with_track(
                            &self.accounts_scroll,
                            "accounts-scrollbar",
                        )),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_preference_tab(
        &self,
        view: SidebarView,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.sidebar_view == view;
        div()
            .id(format!("sidebar-{label}-tab"))
            .debug_selector(move || format!("sidebar-{label}-tab"))
            .flex_none()
            .border_1()
            .border_color(Theme::global().PANEL_BORDER)
            .px_2()
            .py_1()
            .rounded_t_md()
            .h(px(30.0))
            .flex()
            .items_center()
            .when(selected, |el| {
                el.h(px(34.0)).border_b_0().bg(Theme::global().HEADER_BG)
            })
            .cursor_pointer()
            .text_size(px(11.0))
            .text_color(if selected {
                Theme::global().TEXT
            } else {
                Theme::global().TEXT_DIM
            })
            .hover(|el| el.bg(Theme::global().HEADER_BG))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.sidebar_view = view;
                    cx.notify();
                }),
            )
            .child(label)
            .into_any_element()
    }

    pub(super) fn render_theme_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = Theme::active_preset();
        let mut picker = div()
            .id("theme-settings")
            .debug_selector(|| "theme-settings".into())
            .p_3()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(11.0))
            .child(div().text_size(px(14.0)).child("Theme"))
            .child(
                div()
                    .text_color(Theme::global().TEXT_DIM)
                    .child("Choose a palette. Changes save automatically."),
            );
        for (index, preset) in ThemePreset::ALL.into_iter().enumerate() {
            picker = picker.child(
                div()
                    .id(("theme-preset", index))
                    .debug_selector(move || format!("theme-preset-{index}"))
                    .flex_none()
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(if preset == active {
                        Theme::global().ACCENT
                    } else {
                        Theme::global().TEXT
                    })
                    .when(preset == active, |el| el.bg(Theme::global().ACCENT_DIM))
                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.select_theme(preset, cx)),
                    )
                    .child(if preset == active {
                        format!("{}  ✓", preset.label())
                    } else {
                        preset.label().to_owned()
                    }),
            );
        }
        picker.into_any_element()
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut settings = div()
            .id("workspace-settings")
            .debug_selector(|| "workspace-settings".into())
            .p_3()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(11.0))
            .child(div().text_size(px(14.0)).child("Settings"))
            .when(!self.single_panel, |el| {
                el.child(
                    div()
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Current window"),
                )
            });
        for (id, label, enabled) in [
            ("settings-minimap", "Workspace minimap", self.show_minimap),
            (
                "settings-showcase",
                "Show pressed shortcuts",
                self.showcase_mode,
            ),
        ]
        .into_iter()
        .filter(|_| !self.single_panel)
        {
            settings = settings.child(
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .flex_none()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .cursor_pointer()
                    .bg(Theme::global().PANEL_BG)
                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if id == "settings-minimap" {
                                this.show_minimap = !this.show_minimap;
                                cx.notify();
                            } else {
                                this.toggle_showcase(&ToggleShowcase, window, cx);
                            }
                        }),
                    )
                    .child(label)
                    .child(
                        div()
                            .text_color(if enabled {
                                Theme::global().ACCENT
                            } else {
                                Theme::global().TEXT_DIM
                            })
                            .child(if enabled { "On" } else { "Off" }),
                    ),
            );
        }
        if !self.single_panel {
            let folder_tabs = self.layout_mode == crate::config::LayoutMode::FolderTabs;
            settings = settings.child(
                div()
                    .id("settings-folder-tabs")
                    .debug_selector(|| "settings-folder-tabs".into())
                    .flex_none()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .cursor_pointer()
                    .bg(Theme::global().PANEL_BG)
                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.layout_mode =
                                if this.layout_mode == crate::config::LayoutMode::FolderTabs {
                                    crate::config::LayoutMode::Normal
                                } else {
                                    crate::config::LayoutMode::FolderTabs
                                };
                            this.camera_dirty.fill(true);
                            if let Err(error) = crate::config::persist_layout_mode(this.layout_mode)
                            {
                                eprintln!("failed to persist workspace layout mode: {error}");
                            }
                            cx.notify();
                        }),
                    )
                    .child("Workspace layout")
                    .child(
                        div()
                            .text_color(if folder_tabs {
                                Theme::global().ACCENT
                            } else {
                                Theme::global().TEXT_DIM
                            })
                            .child(if folder_tabs { "Folder tabs" } else { "Normal" }),
                    ),
            );
        }
        let mut send_mode_setting = None;
        if let Some(panel) = self
            .slots
            .get(self.active)
            .and_then(|slot| slot.panel.read_chat(cx))
        {
            let after_yield = panel.default_send_after_yield();
            let mut choices = div().flex().gap_1();
            for (id, label, value) in [
                ("settings-send-steer", "Steer", false),
                ("settings-send-after-yield", "After yield", true),
            ] {
                choices = choices.child(
                    div()
                        .id(id)
                        .debug_selector(move || id.into())
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(if after_yield == value {
                            Theme::global().ACCENT
                        } else {
                            Theme::global().TEXT_DIM
                        })
                        .when(after_yield == value, |el| el.bg(Theme::global().ACCENT_DIM))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(slot) = this.slots.get(this.active) {
                                    slot.panel.update_chat(cx, |panel, cx| {
                                        panel.set_default_send_after_yield(value, cx)
                                    });
                                }
                                cx.notify();
                            }),
                        )
                        .child(label),
                );
            }
            send_mode_setting = Some(
                div()
                    .flex_none()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .bg(Theme::global().PANEL_BG)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Default send mode (current session)")
                    .child(choices),
            );
        }
        let applet_settings = self.render_applet_settings(cx);
        let about = self.render_about_settings(cx);
        let clean_chat = crate::config::display().clean_chat_default;
        let clean_chat_setting = crate::keyboard::button(
            div()
                .id("settings-clean-chat")
                .debug_selector(|| "settings-clean-chat".into())
                .flex_none()
                .px_3()
                .py_2()
                .rounded_full()
                .flex()
                .justify_between()
                .gap_2()
                .cursor_pointer()
                .bg(Theme::global().PANEL_BG)
                .hover(|el| el.bg(Theme::global().TOOL_BG))
                .child("Clean chat by default")
                .child(
                    div()
                        .text_color(if clean_chat {
                            Theme::global().ACCENT
                        } else {
                            Theme::global().TEXT_DIM
                        })
                        .child(if clean_chat { "On" } else { "Off" }),
                ),
        )
        .on_click(cx.listener(|_, _, _, cx| {
            match crate::config::set_display("clean_chat_default", "toggle") {
                Ok(_) => cx.refresh_windows(),
                Err(error) => eprintln!("{error}"),
            }
            cx.notify();
        }));
        settings
            .child(self.render_account_settings(cx))
            .child(self.render_account_email_setting(cx))
            .child(self.render_sound_settings(cx))
            .child(self.render_voice_settings(cx))
            .children(applet_settings)
            .when(!self.single_panel, |el| {
                el.child(
                    div()
                        .id("settings-machines")
                        .debug_selector(|| "settings-machines".into())
                        .px_2()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(Theme::global().PANEL_BG)
                        .hover(|el| el.bg(Theme::global().TOOL_BG))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.open_machines(window, cx);
                            }),
                        )
                        .child("Machines and default new-panel location →"),
                )
            })
            .child(clean_chat_setting)
            .when(!self.single_panel, |el| {
                el.child(
                    div()
                        .mt_2()
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Color palettes are in the Theme tab."),
                )
            })
            .children(send_mode_setting)
            .child(about)
            .into_any_element()
    }
}
