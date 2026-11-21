use gpui::{Context, Entity, FontWeight, Window, div, prelude::*, px};
use jcode_desktop_openrig::{
    HostTarget,
    attention::Action,
    inbox::{Inbox, Item, ItemId},
};

use super::{Panel, State, Theme, default_base_url};
use crate::{input::PromptInput, ui_actions::OpenFleetSession};

#[derive(Default)]
pub(super) struct ActionState {
    phase: Phase,
    pub(super) revision: u64,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Confirm {
        item: ItemId,
        action: Action,
        target: Box<HostTarget>,
        input: Option<Entity<PromptInput>>,
    },
    Running {
        item: ItemId,
    },
    Finished {
        item: ItemId,
        result: Result<(), String>,
    },
}

impl ActionState {
    pub(super) fn running(&self) -> bool {
        matches!(self.phase, Phase::Running { .. })
    }

    pub(super) fn cancel(&mut self) -> bool {
        if matches!(self.phase, Phase::Confirm { .. }) {
            self.phase = Phase::Idle;
            true
        } else {
            false
        }
    }

    pub(super) fn handle_key(&mut self, key: &str) -> bool {
        if matches!(self.phase, Phase::Confirm { .. }) {
            if key == "escape" {
                self.cancel();
            }
            return true;
        }
        false
    }

    pub(super) fn keep_confirmation_for(&mut self, inbox: Option<&Inbox>) {
        if let Phase::Confirm { item, .. } = &self.phase
            && inbox.is_some_and(|inbox| {
                inbox.waits_loaded() && !inbox.items.iter().any(|row| &row.id == item)
            })
        {
            self.phase = Phase::Finished {
                item: item.clone(),
                result: Err(
                    "This item is no longer in the latest OpenRig list. Nothing was sent.".into(),
                ),
            };
        }
    }

    fn error(&self, id: &ItemId) -> Option<&str> {
        match &self.phase {
            Phase::Finished {
                item,
                result: Err(error),
            } if item == id => Some(error),
            _ => None,
        }
    }

    pub(super) fn notice(&self, inbox: Option<&Inbox>) -> Option<(&str, bool)> {
        match &self.phase {
            Phase::Running { .. } => Some(("Waiting for OpenRig and refreshing…", false)),
            Phase::Finished { item, result }
                if inbox.is_none_or(|inbox| inbox.items.iter().all(|row| &row.id != item)) =>
            {
                Some(match result {
                    Ok(()) => ("OpenRig accepted the action. List refreshed.", false),
                    Err(error) => (error, true),
                })
            }
            _ => None,
        }
    }
}

impl State {
    pub(super) fn configure_attention_capture(&mut self) {
        let mode = std::env::var("JCODE_DESKTOP_SCREENSHOT_OPENRIG_ATTENTION");
        match mode.as_deref() {
            Ok("live") => {
                self.snapshot = jcode_desktop_openrig::fetch_snapshot(&default_base_url());
                self.snapshot.load_attention();
                self.attention.show_sources = true;
                self.events.clear();
            }
            Ok("inspector" | "confirm") => {
                if let Some(row) = self
                    .snapshot
                    .inbox
                    .iter()
                    .flat_map(|inbox| &inbox.items)
                    .find(|row| !row.actions().is_empty())
                {
                    self.attention.selected = Some(row.id.clone());
                    if mode.as_deref() == Ok("confirm")
                        && let Some(target) = self.snapshot.attention_target(&row.id).cloned()
                    {
                        self.attention_action.phase = Phase::Confirm {
                            item: row.id.clone(),
                            action: row.actions()[0].clone(),
                            target: Box::new(target),
                            input: None,
                        };
                    }
                }
            }
            _ => return,
        }
        if let Some(index) = self
            .snapshot
            .sections
            .iter()
            .position(|section| section.kind == jcode_desktop_openrig::SectionKind::Needs)
        {
            self.section = index;
        }
    }
}

fn pill(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .rounded_full()
        .px_3()
        .py_1()
        .text_size(px(12.))
        .cursor_pointer()
        .bg(if primary {
            theme.ACCENT_DIM
        } else {
            theme.PANEL_BG
        })
        .hover(|style| style.bg(theme.ACCENT_DIM))
        .child(label)
}

impl Panel {
    pub(super) fn render_attention_actions(
        &self,
        row: &Item,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let openrig = self.openrig.as_ref().expect("OpenRig panel");
        let state = &openrig.attention_action;
        let mut content = div().flex().flex_col().gap_2();
        if let Some(session) = &row.session {
            let action = OpenFleetSession {
                target: session.target.clone(),
                native_id: session.native_id.clone(),
            };
            content = content.child(
                div().flex().child(
                    pill(
                        theme,
                        "openrig-attention-session",
                        "Open Jcode session",
                        false,
                    )
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(action.clone()), cx)
                    }),
                ),
            );
        }
        match &state.phase {
            Phase::Confirm {
                item,
                action,
                target,
                input,
            } if item == &row.id => {
                let stale = !row.actions().contains(action)
                    || openrig.snapshot.attention_target(item) != Some(target.as_ref());
                return content.child(
                    div()
                        .id("openrig-attention-confirmation")
                        .debug_selector(|| "openrig-attention-confirmation".into())
                        .flex()
                        .flex_col()
                        .gap_2()
                        .capture_action(
                            cx.listener(|_, _: &crate::input::Submit, _, cx| cx.stop_propagation()),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Confirm: {}", action.label())),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.TEXT_DIM)
                                .child(action.effect()),
                        )
                        .when_some(input.clone(), |el, input| el.child(input))
                        .when(stale, |el| {
                            el.child(
                                div().text_color(theme.WARN).child(
                                    "This item changed. Cancel and review the latest details.",
                                ),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    pill(theme, "openrig-attention-cancel", "Cancel", false)
                                        .debug_selector(|| "openrig-attention-cancel".into())
                                        .on_click(cx.listener(|panel, _, window, cx| {
                                            panel.cancel_attention_action(window, cx)
                                        })),
                                )
                                .when(!stale, |el| {
                                    el.child(
                                        pill(theme, "openrig-attention-confirm", "Confirm", true)
                                            .debug_selector(|| "openrig-attention-confirm".into())
                                            .on_click(cx.listener(|panel, _, window, cx| {
                                                panel.confirm_attention_action(window, cx)
                                            })),
                                    )
                                }),
                        ),
                );
            }
            Phase::Running { item } if item == &row.id => {
                return content.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.TEXT_DIM)
                        .child("Waiting for OpenRig and refreshing…"),
                );
            }
            Phase::Finished {
                item,
                result: Ok(()),
            } if item == &row.id => {
                content = content.child(
                    div()
                        .text_color(theme.OK)
                        .text_size(px(12.))
                        .child("OpenRig accepted the action. Item refreshed."),
                );
            }
            _ => {}
        }
        if let Some(error) = state.error(&row.id) {
            content = content.child(
                div()
                    .id("openrig-attention-action-error")
                    .debug_selector(|| "openrig-attention-action-error".into())
                    .text_size(px(12.))
                    .text_color(theme.ERROR)
                    .child(error.to_owned()),
            );
        }
        if state.running() || row.actions().is_empty() {
            return content;
        }
        let buttons = row
            .actions()
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, action)| {
                let item = row.id.clone();
                let label = action.label().to_owned();
                div()
                    .id(("openrig-attention-action", index))
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .bg(theme.PANEL_BG)
                    .hover(|style| style.bg(theme.ACCENT_DIM))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .child(label)
                    .debug_selector(move || format!("openrig-attention-action:{index}"))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.stage_attention_action(item.clone(), action.clone(), window, cx)
                    }))
            });
        content.child(div().flex().flex_wrap().gap_2().children(buttons))
    }

    fn stage_attention_action(
        &mut self,
        item: ItemId,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.openrig.as_mut() else {
            return;
        };
        if state.attention_action.running() {
            return;
        }
        let Some(target) = state.snapshot.attention_target(&item).cloned() else {
            return;
        };
        let input = action.input_label().map(|label| {
            let weak = cx.weak_entity();
            cx.new(|cx| {
                PromptInput::new(cx, label, |_, _, _, _| {})
                    .without_command_completion()
                    .with_on_overlay_cancel(move |app| {
                        let _ = weak.update(app, |panel, cx| {
                            if let Some(state) = panel.openrig.as_mut() {
                                state.attention_action.cancel();
                            }
                            cx.notify();
                        });
                        true
                    })
            })
        });
        if let Some(input) = &input {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        state.attention_action.phase = Phase::Confirm {
            item,
            action,
            target: Box::new(target),
            input,
        };
        cx.notify();
    }

    fn cancel_attention_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.openrig.as_mut() {
            state.attention_action.cancel();
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn confirm_attention_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.openrig.as_mut() else {
            return;
        };
        let Phase::Confirm {
            item,
            action,
            target,
            input,
        } = &state.attention_action.phase
        else {
            return;
        };
        let offered = state.snapshot.inbox.as_ref().is_some_and(|inbox| {
            inbox
                .items
                .iter()
                .any(|row| &row.id == item && row.actions().contains(action))
        });
        if !offered || state.snapshot.attention_target(item) != Some(target.as_ref()) {
            return;
        }
        let text = input
            .as_ref()
            .map_or("", |input| input.read(cx).content.as_str());
        let Some(confirmed) = action.confirm(text) else {
            if let Some(input) = input {
                window.focus(&input.read(cx).focus_handle.clone(), cx);
            }
            return;
        };
        let target = target.as_ref().clone();
        let item = item.clone();
        state.attention_action.revision += 1;
        state.attention_action.phase = Phase::Running { item: item.clone() };
        window.focus(&self.focus_handle, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (result, snapshot) = cx
                .background_executor()
                .spawn(async move {
                    // Fixtures never send mutations, even if the synthetic Confirm button is clicked.
                    if cfg!(test) || crate::harness::screenshot_mode() {
                        return (Err("This fixture refused the request.".into()), None);
                    }
                    let base = default_base_url();
                    let result = confirmed.execute(&target);
                    let mut snapshot = jcode_desktop_openrig::fetch_snapshot(&base);
                    snapshot.load_attention();
                    (result, Some(snapshot))
                })
                .await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(state) = panel.openrig.as_mut() {
                    state.attention_action.revision += 1;
                    state.attention_action.phase = Phase::Finished { item, result };
                    if let Some(snapshot) = snapshot {
                        state.snapshot.fleet = snapshot.fleet;
                        state.snapshot.inbox = snapshot.inbox;
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
