//! The Accounts picker's row views: one pill per login with its chips, limits
//! and banked resets, drag handles, and the provider each row selects.
//! `accounts` owns what the rows say, this module only draws them, and
//! `provider_picker` lays them out.
use super::accounts::*;
use super::connection::ConnectionStatus;
use super::*;
use crate::accounts::{BankedReset, UsageLimit};
use std::rc::Rc;

impl Panel {
    fn picker_choice(&self, row: &AccountRow, cx: &App) -> PickerChoice {
        let session = session_provider_id(self.provider.as_deref(), self.auth_method.as_deref());
        let has_route = self
            .input
            .read(cx)
            .provider_route_spec(row.provider.id, self.model.as_deref())
            .is_some();
        picker_choice_for(session.as_deref(), row.provider.id, row.status, has_route)
    }
}

impl Panel {
    /// The Default chip follows the saved default route, so it stays right
    /// whichever path set it. With nothing saved, #1 in Auto-switch leads.
    fn is_default_account(&self, key: &str, position: Option<usize>) -> bool {
        let Some(state) = self.login.as_ref() else {
            return false;
        };
        match &state.accounts.default_key {
            Some(default) => default == key,
            None => state.accounts.default_route.is_none() && position == Some(0),
        }
    }

    pub(super) fn render_account_row(
        &self,
        row: &AccountRow,
        pooled: Option<bool>,
        visible: &Rc<Vec<String>>,
        position: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pick = self
            .login
            .as_ref()
            .filter(|state| state.selection_only)
            .map(|_| self.picker_choice(row, cx));
        AccountRowView {
            row,
            pooled,
            visible,
            position,
            pick,
            default: pick.is_none() && self.is_default_account(&row.key, position),
            live_error: self
                .login
                .as_ref()
                .and_then(|state| state.live_errors.get(row.provider.id))
                .map(String::as_str),
        }
        .render(Self::handle_account_row, cx)
    }

    fn handle_account_row(
        &mut self,
        action: RowAction<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            RowAction::Redeem(reset) => cx.emit(AccountsPanelRedeemReset(reset.clone())),
            RowAction::Place {
                key,
                pooled,
                before,
                visible,
            } => self.place_account(key, pooled, before, visible, cx),
            RowAction::Test(provider) => self.live_test_providers(Some(provider.to_owned()), cx),
            RowAction::Choose {
                provider,
                label,
                switching,
            } => {
                if switching {
                    self.select_existing_provider(provider, label.clone(), window, cx);
                } else {
                    self.select_login_provider_for(provider, label.clone(), cx);
                }
            }
        }
    }
}

enum RowAction<'a> {
    Redeem(&'a BankedReset),
    Place {
        key: &'a str,
        pooled: bool,
        before: Option<&'a str>,
        visible: &'a [String],
    },
    Test(&'a str),
    Choose {
        provider: LoginProvider,
        label: &'a Option<String>,
        switching: bool,
    },
}

type RowHandler<T> = fn(&mut T, RowAction<'_>, &mut Window, &mut Context<T>);

struct AccountRowView<'a> {
    row: &'a AccountRow,
    pooled: Option<bool>,
    visible: &'a Rc<Vec<String>>,
    position: Option<usize>,
    pick: Option<PickerChoice>,
    default: bool,
    live_error: Option<&'a str>,
}

impl AccountRowView<'_> {
    /// The saved resets a login can use right now. Clicking only opens the
    /// workspace review, which prepares against the provider and needs an
    /// explicit confirm. A login with none gets no chip.
    fn render_banked_reset<T: 'static>(
        key: &str,
        reset: &BankedReset,
        handle: RowHandler<T>,
        cx: &mut Context<T>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let id = format!("login-reset-{key}");
        let selector = id.clone();
        let color = theme.ACCENT;
        let reset = reset.clone();
        chip_base(color)
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .child(icon_reset(color))
            .child(reset_pill_label(&reset))
            // A reset chip must not fall through to the row's sign-in.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .cursor_pointer()
            .hover(move |el| el.bg(color.opacity(0.24)))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                handle(this, RowAction::Redeem(&reset), window, cx);
            }))
            .into_any_element()
    }
}

impl Panel {
    fn place_account(
        &mut self,
        key: &str,
        pooled: bool,
        before: Option<&str>,
        visible: &[String],
        cx: &mut Context<Self>,
    ) {
        // Picking up and releasing the same account is a no-op, not an
        // implicit move to the end after filtering it out of `remaining`.
        if before == Some(key) {
            return;
        }
        let offline =
            self.preview_state.is_some() || cfg!(test) || crate::harness::screenshot_mode();
        let Some(state) = self.login.as_mut() else {
            return;
        };
        let remaining: Vec<&str> = visible
            .iter()
            .map(String::as_str)
            .filter(|member| *member != key)
            .collect();
        let index = before
            .and_then(|target| remaining.iter().position(|member| *member == target))
            .unwrap_or(remaining.len());
        let visible: Vec<&str> = visible.iter().map(String::as_str).collect();
        state.accounts.pool.place(key, pooled, index, &visible);
        // The first auto-switch account is the default for new sessions.
        let first = state
            .accounts
            .pool
            .members
            .first()
            .filter(|first| visible.first() != Some(&first.as_str()))
            .cloned();
        if let Some(first) = &first
            && let Some(route) = jcode_base::auth::account_pool::default_route_for_key(first)
        {
            state.accounts.default_route = Some(route.to_owned());
            state.accounts.default_key = Some(first.clone());
        }
        if !offline {
            let pool = state.accounts.pool.clone();
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = pool.save() {
                        eprintln!("[accounts] could not save auto-switch order: {error}");
                    }
                    if let Some(first) = first
                        && let Err(error) =
                            jcode_base::auth::account_pool::apply_default_account(&first)
                    {
                        eprintln!("[accounts] could not make {first} the default: {error}");
                    }
                })
                .detach();
        }
        cx.notify();
    }
}

impl AccountRowView<'_> {
    fn render<T: 'static>(&self, handle: RowHandler<T>, cx: &mut Context<T>) -> AnyElement {
        let Self {
            row,
            pooled,
            visible,
            position,
            pick,
            default,
            live_error,
        } = *self;
        let selecting = pick.is_some();
        let theme = Theme::global();
        let provider = row.provider;
        let status = row.status;
        let color = status.color();
        let key = row.key.clone();
        let row_id = format!("login-account-{key}");
        let provider_selector = if row.first_of_provider {
            format!("login-provider-{}", provider.id)
        } else {
            format!(
                "login-provider-{}-{}",
                provider.id,
                row.label.as_deref().unwrap_or("")
            )
        };
        let status_selector = if row.first_of_provider {
            format!("login-status-{}", provider.id)
        } else {
            format!("login-status-{key}")
        };
        let title = provider.display_name.to_owned();
        let mut name_line = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_2()
            .gap_y_1()
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .text_size(px(14.))
                    .text_color(theme.TEXT)
                    .whitespace_nowrap()
                    .child(title.clone()),
            )
            .child(div().flex_none().child(login_method_icon(provider.method)));
        if row.active && !selecting {
            name_line = name_line.child(chip("In use", theme.ACCENT));
        }
        if default {
            name_line = name_line.child(
                chip("Default", theme.ACCENT).debug_selector(|| "login-default-account".into()),
            );
        }
        if let Some(plan) = &row.plan {
            name_line = name_line.child(chip(plan.clone(), theme.TEXT_DIM));
        }
        if let Some(reset) = row.banked.as_ref().filter(|reset| reset.usable() > 0) {
            name_line = name_line.child(Self::render_banked_reset(&row.key, reset, handle, cx));
        }
        let detail: Vec<String> = [row.subtitle(), row.usage.clone()]
            .into_iter()
            .flatten()
            .collect();
        let mut text = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(name_line);
        if !detail.is_empty() {
            text = text.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_DIM)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(detail.join(" · ")),
            );
        }
        let connected = pooled.is_some();
        let live_error = live_error.filter(|_| {
            status == ConnectionStatus::Failed || status == ConnectionStatus::Unverified
        });
        // A provider that rejected the key says why on the row itself, so the
        // user does not have to press Test to learn what went wrong.
        let usage_error = (status == ConnectionStatus::Failed)
            .then_some(row.error.as_deref())
            .flatten();
        if let Some(error) = live_error.or(usage_error) {
            text = text.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.ERROR)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(first_line(error)),
            );
        }
        // Meters sit on their own line inside the text column. Beside the
        // name they overlapped the plan and reset chips on narrow panels.
        if !row.limits.is_empty() {
            let mut meters = div().flex().items_center().gap_4().w_full().pt(px(2.));
            // OpenCode reports three windows (rolling, weekly, monthly); every
            // reported one stays visible rather than silently dropping the
            // monthly cap.
            for limit in row.limits.iter().take(3) {
                meters = meters.child(limit_meter(limit));
            }
            text = text.child(meters);
        }
        let mut element = div()
            .id(SharedString::from(row_id.clone()))
            .debug_selector(move || provider_selector.clone())
            .flex()
            .items_center()
            .gap(px(if selecting { 8. } else { 12. }))
            .pl(px(if selecting { 8. } else { 10. }))
            .pr(px(if selecting { 8. } else { 10. }))
            .py(px(if selecting { 4. } else { 6. }))
            .min_h(px(match (selecting, connected) {
                (true, _) => 36.,
                (false, true) => 56.,
                (false, false) => 44.,
            }))
            .rounded(px(if selecting { 8. } else { 12. }))
            // Picker rows are all connected credentials: full strength unless
            // the picker marks one unavailable below.
            .bg(if connected || selecting {
                theme.HEADER_BG
            } else {
                theme.wash(crate::theme::Ink::HeaderBg, 0.55)
            })
            .cursor_pointer()
            .hover(|el| el.bg(theme.ACCENT_DIM));
        if connected {
            let handle_id = format!("login-drag-handle-{key}");
            let selector = handle_id.clone();
            let dragged = DraggedAccount {
                key: key.clone(),
                title: match &row.label {
                    Some(label) => format!("{title} · {label}"),
                    None => title.clone(),
                },
            };
            element = element.child(
                div()
                    .id(SharedString::from(handle_id))
                    .debug_selector(move || selector.clone())
                    .flex_none()
                    .w(px(28.))
                    .h(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .rounded_full()
                    .cursor(gpui::CursorStyle::OpenHand)
                    .hover(|el| el.bg(theme.ACCENT_DIM))
                    .children(
                        (0..3)
                            .map(|_| div().w(px(14.)).h(px(2.)).rounded_full().bg(theme.TEXT_DIM)),
                    )
                    .tooltip(|_, cx| {
                        cx.new(|_| {
                            super::meter::MeterTooltip(
                                "Drag to reorder or move between Auto-switch and Manual".into(),
                            )
                        })
                        .into()
                    })
                    .on_drag(dragged, |dragged, _, _, cx| {
                        cx.new(|_| AccountDragPreview(dragged.title.clone()))
                    })
                    // A grip is not a sign-in button, even without movement.
                    .on_click(|_, _, cx| cx.stop_propagation()),
            );
        }
        // Auto-switch order reads left to right, before the provider.
        if let Some(position) = position {
            let mut controls = div().flex().flex_col().flex_none().gap(px(2.));
            for (direction, label, enabled, target) in [
                ("up", "↑", position > 0, position.saturating_sub(1)),
                ("down", "↓", position + 1 < visible.len(), position + 2),
            ] {
                let id = format!("login-order-{direction}-{key}");
                let selector = id.clone();
                let move_key = key.clone();
                let visible = visible.clone();
                let before = visible.get(target).cloned();
                controls = controls.child(
                    div()
                        .id(SharedString::from(id))
                        .debug_selector(move || selector.clone())
                        .w(px(28.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.PANEL_BG)
                        .text_size(px(14.))
                        .text_color(theme.TEXT_DIM)
                        .when(enabled, |el| {
                            el.cursor_pointer()
                                .hover(|el| el.bg(theme.ACCENT_DIM).text_color(theme.ACCENT))
                        })
                        .when(!enabled, |el| el.opacity(0.3))
                        .child(label)
                        .tooltip(move |_, cx| {
                            cx.new(|_| {
                                super::meter::MeterTooltip(format!(
                                    "Move {direction} in auto-switch order"
                                ))
                            })
                            .into()
                        })
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            if enabled {
                                handle(
                                    this,
                                    RowAction::Place {
                                        key: &move_key,
                                        pooled: true,
                                        before: before.as_deref(),
                                        visible: &visible,
                                    },
                                    window,
                                    cx,
                                );
                            }
                        })),
                );
            }
            element = element.child(controls);
            element = element.child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.wash(crate::theme::Ink::Accent, 0.16))
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ACCENT)
                    .child(format!("{}", position + 1)),
            );
        } else if connected {
            // Manual rows keep the same logo column as ordered rows.
            element = element.child(div().flex_none().size(px(20.)));
        }
        element = element
            .child(
                div()
                    .flex_none()
                    .size(px(if connected { 34. } else { 28. }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.PANEL_BG)
                    .child(login_logo(&provider)),
            )
            .child(text);
        if connected || status != ConnectionStatus::NotConnected {
            let tip = status.detail();
            element = element.child(
                status_chip(status.label(), color)
                    .id(SharedString::from(format!("login-status-chip-{key}")))
                    .debug_selector(move || status_selector.clone())
                    .tooltip(move |_, cx| {
                        cx.new(|_| super::meter::MeterTooltip(tip.into())).into()
                    }),
            );
        } else {
            element = element.child(
                chip_base(theme.ACCENT)
                    .debug_selector(move || status_selector.clone())
                    .child("Connect"),
            );
        }
        if connected {
            let test_id = format!("login-test-{key}");
            let selector = test_id.clone();
            let provider_id = provider.id.to_owned();
            let busy = status == ConnectionStatus::Testing;
            element = element.child(
                div()
                    .id(SharedString::from(test_id))
                    .debug_selector(move || selector.clone())
                    .flex_none()
                    .px_2p5()
                    .py(px(3.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.PANEL_BORDER)
                    .text_size(px(11.))
                    .text_color(theme.TEXT_DIM)
                    .whitespace_nowrap()
                    .when(!busy, |el| {
                        el.cursor_pointer()
                            .hover(|el| el.bg(theme.ACCENT_DIM).text_color(theme.TEXT))
                    })
                    .when(busy, |el| el.opacity(0.5))
                    .child("Test")
                    .tooltip(|_, cx| {
                        cx.new(|_| {
                            super::meter::MeterTooltip("Test account · Uses quota or credit".into())
                        })
                        .into()
                    })
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        handle(this, RowAction::Test(provider_id.as_str()), window, cx)
                    })),
            );
        }
        let label = row.label.clone();
        match pick {
            Some(PickerChoice::Current) => {
                // The active account stands out: accent outline and fill.
                element = element
                    .border_1()
                    .border_color(theme.ACCENT)
                    .bg(theme.wash(crate::theme::Ink::Accent, 0.14))
                    .cursor_default()
                    .child(
                        chip("In use", theme.ACCENT)
                            .debug_selector(move || format!("picker-active-{}", provider.id)),
                    );
            }
            Some(PickerChoice::Unavailable(reason)) => {
                element = element.opacity(0.5).cursor_default().child(
                    chip(reason, theme.TEXT_DIM)
                        .debug_selector(move || format!("picker-unavailable-{}", provider.id)),
                );
            }
            Some(PickerChoice::Switch) => {
                element = element.child(
                    div().debug_selector(move || format!("picker-available-{}", provider.id)),
                );
            }
            None => {}
        }
        if pick.is_some_and(|pick| pick != PickerChoice::Switch) {
            // Nothing to do: the row is already in use or has no route.
            return element.into_any_element();
        }
        element = element.on_click(cx.listener(move |this, _, window, cx| {
            handle(
                this,
                RowAction::Choose {
                    provider,
                    label: &label,
                    switching: selecting,
                },
                window,
                cx,
            );
        }));
        if let Some(pooled) = pooled {
            let before = key.clone();
            let visible = visible.clone();
            element = element
                .on_drag(
                    DraggedAccount {
                        key: key.clone(),
                        title: match &row.label {
                            Some(label) => format!("{title} · {label}"),
                            None => title,
                        },
                    },
                    |dragged, _, _, cx| cx.new(|_| AccountDragPreview(dragged.title.clone())),
                )
                .drag_over::<DraggedAccount>(|style, _, _, _| style.bg(Theme::global().ACCENT_DIM))
                .on_drop(
                    cx.listener(move |this, dragged: &DraggedAccount, window, cx| {
                        handle(
                            this,
                            RowAction::Place {
                                key: &dragged.key,
                                pooled,
                                before: Some(&before),
                                visible: &visible,
                            },
                            window,
                            cx,
                        )
                    }),
                );
        }
        element.into_any_element()
    }
}

impl Panel {
    #[allow(clippy::too_many_arguments)]
    fn render_account_group(
        &self,
        id: &'static str,
        title: &'static str,
        hint: &'static str,
        rows: &[AccountRow],
        pooled: bool,
        visible: &Rc<Vec<String>>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let mut group = div()
            .id(id)
            .debug_selector(move || id.into())
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_xl()
            .drag_over::<DraggedAccount>(|style, _, _, _| {
                style.bg(Theme::global().wash(crate::theme::Ink::AccentDim, 0.35))
            })
            .on_drop({
                let visible = visible.clone();
                cx.listener(move |this, dragged: &DraggedAccount, _, cx| {
                    this.place_account(&dragged.key, pooled, None, &visible, cx)
                })
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px_2()
                    .pb_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.TEXT)
                                    .child(title),
                            )
                            .child(chip(rows.len().to_string(), theme.TEXT_DIM)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .child(hint),
                    ),
            );
        if rows.is_empty() {
            group = group.child(
                div()
                    .px_3()
                    .py_3()
                    .rounded_full()
                    .bg(theme.wash(crate::theme::Ink::HeaderBg, 0.5))
                    .text_size(px(12.))
                    .text_color(theme.TEXT_DIM)
                    .child("Drag an account here"),
            );
        }
        for (index, row) in rows.iter().enumerate() {
            group = group.child(self.render_account_row(
                row,
                Some(pooled),
                visible,
                pooled.then_some(index),
                cx,
            ));
        }
        group.into_any_element()
    }

    pub(super) fn render_account_catalog(
        &self,
        providers: &[LoginProvider],
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let Some(state) = self.login.as_ref() else {
            return Vec::new();
        };
        let theme = Theme::global();
        let rows = state.account_rows(providers);
        let groups = group_rows(rows, &state.accounts.pool);
        let visible = Rc::new(
            groups
                .pooled
                .iter()
                .map(|row| row.key.clone())
                .collect::<Vec<_>>(),
        );
        let mut out = Vec::new();
        if !groups.pooled.is_empty() || !groups.manual.is_empty() {
            out.push(self.render_account_group(
                "login-group-auto",
                "Auto-switch",
                "The first account is the default for new sessions. The rest take over in order when one runs out or fails. Drag the ≡ handle or use ↑ ↓ to reorder.",
                &groups.pooled,
                true,
                &visible,
                cx,
            ));
            out.push(self.render_account_group(
                "login-group-manual",
                "Manual",
                "Never switched to automatically. Used only when you pick it.",
                &groups.manual,
                false,
                &visible,
                cx,
            ));
        }
        if !groups.available.is_empty() || !groups.addable.is_empty() {
            let mut section = div()
                .debug_selector(|| "login-group-available".into())
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .child(
                    div()
                        .px_2()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Connect an account"),
                );
            if !groups.addable.is_empty() {
                let mut adds = div().flex().flex_wrap().gap_2().px_1();
                for provider in groups.addable {
                    let id = format!("login-add-{}", provider.id);
                    adds = adds.child(
                        div()
                            .id(SharedString::from(id.clone()))
                            .debug_selector(move || id.clone())
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1p5()
                            .rounded_full()
                            .bg(theme.wash(crate::theme::Ink::Accent, 0.16))
                            .text_color(theme.ACCENT)
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(|el| el.bg(theme.wash(crate::theme::Ink::Accent, 0.26)))
                            .child(format!("+ Another {} account", provider.display_name))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_login_provider_for(
                                    provider,
                                    Some(NEW_ACCOUNT_LABEL.into()),
                                    cx,
                                )
                            })),
                    );
                }
                section = section.child(adds);
            }
            for row in &groups.available {
                section = section.child(self.render_account_row(row, None, &visible, None, cx));
            }
            out.push(section.into_any_element());
        }
        out
    }
}

impl Panel {
    pub(super) fn select_existing_provider(
        &mut self,
        provider: LoginProvider,
        _label: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = self
            .input
            .read(cx)
            .provider_route_spec(provider.id, self.model.as_deref());
        let Some(spec) = spec else {
            if let Some(state) = self.login.as_mut() {
                state.error = Some(format!(
                    "{} has no available model for this session.",
                    provider.display_name
                ));
            }
            cx.notify();
            return;
        };
        self.close_login_picker(cx);
        if self.preview_state.is_none() {
            self.bridge.send(Command::SetModel {
                session_id: self.session_id.clone(),
                model: spec.clone(),
            });
        }
        self.items.push(Item::Assistant(format!(
            "Switching provider to {}…",
            provider.display_name
        )));
        self.focus_input(window, cx);
        cx.notify();
    }
}
struct AccountDragPreview(String);

impl Render for AccountDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("AccountDragPreview");
        let theme = Theme::global();
        div()
            .debug_selector(|| "login-account-drag-preview".into())
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .rounded_full()
            .bg(theme.ACCENT_DIM)
            .border_1()
            .border_color(theme.ACCENT)
            .cursor(gpui::CursorStyle::ClosedHand)
            .text_size(px(13.))
            .text_color(theme.TEXT)
            .child("≡")
            .child(self.0.clone())
    }
}

pub(super) fn short_limit_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.starts_with("5-hour") || lower.starts_with("5 hour") {
        "5h".into()
    } else if lower.starts_with("7-day") || lower.starts_with("7 day") || lower.contains("week") {
        "7d".into()
    } else {
        name.split_whitespace().next().unwrap_or(name).to_owned()
    }
}

fn limit_meter(limit: &UsageLimit) -> gpui::AnyElement {
    let theme = Theme::global();
    let used = limit.usage_percent.clamp(0., 100.);
    let color = crate::account_status::usage_color(used, theme);
    let reading = crate::account_status::configured_usage_reading(used);
    let caption = match &limit.reset_in {
        Some(reset) => format!("{} {reading} · {reset}", short_limit_name(&limit.name)),
        None => format!("{} {reading}", short_limit_name(&limit.name)),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .flex_1()
        .min_w(px(72.))
        .max_w(px(160.))
        .child(
            div()
                .text_size(px(10.))
                .text_color(theme.TEXT_DIM)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(caption),
        )
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .overflow_hidden()
                .bg(theme.INLINE_CODE_BG)
                .child(
                    div()
                        .h_full()
                        .w(relative(used / 100.))
                        .rounded_full()
                        .bg(color),
                ),
        )
        .into_any_element()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).trim().to_owned()
}

fn chip(label: impl Into<SharedString>, color: gpui::Rgba) -> gpui::Div {
    chip_base(color).child(label.into())
}

fn icon_reset(color: gpui::Rgba) -> gpui::Div {
    div().text_size(px(11.)).text_color(color).child("↻")
}

fn status_chip(label: &'static str, color: gpui::Rgba) -> gpui::Div {
    chip_base(color).child(label)
}

fn chip_base(color: gpui::Rgba) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py(px(3.))
        .rounded_full()
        .bg(color.opacity(0.12))
        .text_size(px(11.))
        .text_color(color)
        .whitespace_nowrap()
}
