//! Reasoning effort switching for a chat panel: `/effort`, the effort pill
//! menu, and the fast increase/decrease keys all funnel through here.
//!
//! A switch is shown immediately (the pill moves on the keypress) and then
//! settled by the runtime: a refusal restores the previous level, and the
//! identity broadcast that follows a success carries the level the provider
//! actually applied.
use super::*;
use crate::effort;

/// An effort request the runtime has not answered yet.
#[derive(Clone, Debug)]
pub(super) struct PendingEffort {
    /// What the pill showed before the first unanswered request.
    previous: Option<String>,
    /// Requests sent and not yet settled, oldest first.
    requested: VecDeque<String>,
}

impl Panel {
    /// Levels the serving model accepts.
    pub(super) fn effort_ladder(&self) -> Vec<&'static str> {
        effort::ladder(self.provider.as_deref(), self.model.as_deref())
    }

    /// Keep the composer's `/effort` menu on the serving model's ladder.
    /// Deferred: `/effort` runs inside the composer's own submit callback.
    pub(super) fn sync_effort_menu(&self, cx: &mut Context<Self>) {
        let ladder = self.effort_ladder();
        let current = self.reasoning_effort.clone();
        let input = self.input.clone();
        cx.defer(move |cx| {
            input.update(cx, |input, cx| input.set_effort_state(ladder, current, cx))
        });
    }

    /// Validate and send one effort change. Returns false, after reporting
    /// why in the transcript, when the level is not offered.
    pub(super) fn request_effort(&mut self, level: &str, cx: &mut Context<Self>) -> bool {
        let ladder = self.effort_ladder();
        self.request_effort_on(&ladder, level, cx)
    }

    /// Switch model and effort together, from the picker's second step. The
    /// level is checked against the new model's ladder, not the serving one.
    pub(super) fn switch_model_with_effort(
        &mut self,
        model: &str,
        level: &str,
        cx: &mut Context<Self>,
    ) {
        let ladder = effort::ladder(None, Some(model));
        if self.is_pending_session() {
            self.pick_draft_model(model, cx);
        } else {
            self.bridge.send(Command::SetModel {
                session_id: self.session_id.clone(),
                model: model.to_string(),
            });
            self.items
                .push(Item::Assistant(format!("Switching model to `{model}`…")));
        }
        self.request_effort_on(&ladder, level, cx);
    }

    fn request_effort_on(
        &mut self,
        ladder: &[&'static str],
        level: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_side_document() || self.preview_state.is_some() {
            return false;
        }
        if ladder.is_empty() {
            self.items.push(Item::Error(format!(
                "Reasoning effort is not available for {}.",
                self.model.as_deref().unwrap_or("this model")
            )));
            return false;
        }
        let Some(level) = ladder.iter().copied().find(|candidate| *candidate == level) else {
            self.items.push(Item::Error(format!(
                "Usage: `/effort <{}>`",
                ladder.join("|")
            )));
            return false;
        };
        if self.is_pending_session() {
            // A draft has no session to tell: keep the choice for its first
            // prompt.
            self.draft_config.effort = Some(level.to_string());
            self.reasoning_effort = Some(level.to_string());
            self.sync_effort_menu(cx);
            cx.notify();
            return true;
        }
        let pending = self.pending_effort.get_or_insert_with(|| PendingEffort {
            previous: self.reasoning_effort.clone(),
            requested: VecDeque::new(),
        });
        pending.requested.push_back(level.to_string());
        self.reasoning_effort = Some(level.to_string());
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation: SessionOperation::SetEffort(level.to_string()),
        });
        self.sync_effort_menu(cx);
        cx.notify();
        true
    }

    /// One rung up (`direction > 0`) or down, from the fast effort keys.
    pub(super) fn step_effort(&mut self, direction: i8, cx: &mut Context<Self>) {
        let ladder = self.effort_ladder();
        if ladder.is_empty() {
            self.effort_notice("Reasoning effort not available for this model".into(), cx);
            return;
        }
        match effort::step(&ladder, self.reasoning_effort.as_deref(), direction) {
            Some(next) => {
                if self.request_effort(next, cx) {
                    self.effort_notice(format!("Effort: {}", effort::spoken(next)), cx);
                }
            }
            None => {
                let current = self
                    .reasoning_effort
                    .as_deref()
                    .map(effort::spoken)
                    .unwrap_or_else(|| "default".to_string());
                let end = if direction > 0 {
                    "strongest"
                } else {
                    "weakest"
                };
                self.effort_notice(format!("Effort: {current} (already {end})"), cx);
            }
        }
    }

    /// A quiet confirmation under the composer, like the TUI status notice.
    fn effort_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        let input = self.input.clone();
        cx.defer(move |cx| input.update(cx, |input, cx| input.set_transient_notice(notice, cx)));
    }

    /// The runtime answered one effort request.
    pub(crate) fn effort_settled(
        &mut self,
        effort: &str,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_effort.as_mut() else {
            if let Some(error) = error {
                self.items.push(Item::Error(format!(
                    "Failed to set effort `{effort}`: {error}"
                )));
            }
            cx.notify();
            return;
        };
        if let Some(index) = pending.requested.iter().position(|level| level == effort) {
            pending.requested.remove(index);
        }
        let settled = pending.requested.is_empty();
        if let Some(error) = error {
            self.items.push(Item::Error(format!(
                "Failed to set effort `{effort}`: {error}"
            )));
            crate::sounds::play(crate::sounds::Cue::Error, cx);
            if settled {
                // Nothing newer is in flight, so the pill must return to the
                // level the provider is still using.
                self.reasoning_effort = pending.previous.clone();
            }
        }
        if settled {
            self.pending_effort = None;
        }
        self.sync_effort_menu(cx);
        cx.notify();
    }

    /// Identity reported an effort. While requests are in flight, an older
    /// broadcast must not snap the pill back between rapid keypresses.
    pub(super) fn identity_effort(&mut self, reported: Option<String>) {
        match &mut self.pending_effort {
            Some(pending) => {
                // Remember the runtime's truth for a later refusal, but keep
                // showing the newest request.
                pending.previous = reported;
            }
            None => self.reasoning_effort = reported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Command;
    use std::sync::mpsc::Receiver;

    fn setup<'a>(
        cx: &'a mut gpui::TestAppContext,
        provider: &str,
        model: &str,
        effort: Option<&str>,
    ) -> (
        Entity<Panel>,
        &'a mut gpui::VisualTestContext,
        Receiver<Command>,
    ) {
        cx.update(crate::input::bind_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let mut panel = None;
        workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
        let panel = panel.expect("test panel exists");
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some(provider.into()),
                    model: Some(model.into()),
                    reasoning_effort: effort.map(str::to_string),
                    auth_method: None,
                    account_label: None,
                },
                cx,
            );
        });
        vcx.update(|window, cx| {
            let handle = panel.read(cx).input.read(cx).focus_handle.clone();
            window.focus(&handle, cx);
        });
        vcx.run_until_parked();
        (panel, vcx, commands)
    }

    fn sent_efforts(commands: &Receiver<Command>) -> Vec<String> {
        commands
            .try_iter()
            .filter_map(|command| match command {
                Command::SessionOperation {
                    operation: SessionOperation::SetEffort(effort),
                    ..
                } => Some(effort),
                _ => None,
            })
            .collect()
    }

    fn effort_of(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> Option<String> {
        panel.read_with(vcx, |panel, _| panel.reasoning_effort.clone())
    }

    #[gpui::test]
    fn typed_effort_medium_reaches_opus_55_from_enter(cx: &mut gpui::TestAppContext) {
        typed_medium(cx, "claude-oauth", "claude-opus-5-5");
    }

    #[gpui::test]
    fn typed_effort_medium_reaches_sol_from_enter(cx: &mut gpui::TestAppContext) {
        typed_medium(cx, "openai", "gpt-6.1-sol");
    }

    #[gpui::test]
    fn typed_effort_medium_after_pill_click_reaches_opus_55(cx: &mut gpui::TestAppContext) {
        typed_medium_after_pill(cx, "claude-oauth", "claude-opus-5-5");
    }

    #[gpui::test]
    fn typed_effort_medium_after_pill_click_reaches_sol(cx: &mut gpui::TestAppContext) {
        typed_medium_after_pill(cx, "openai", "gpt-6.1-sol");
    }

    fn typed_medium_after_pill(cx: &mut gpui::TestAppContext, provider: &str, model: &str) {
        let (panel, vcx, commands) = setup(cx, provider, model, Some("high"));
        let pill = vcx
            .debug_bounds("panel-model-effort")
            .expect("effort pill painted");
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("picker-search").is_some());
        vcx.simulate_keystrokes("/ e f f o r t space m e d i u m");
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("voice-toggle").is_none(),
            "empty main composer must not advertise example prompts under the picker"
        );
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["medium"]);
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("medium"));
        assert!(
            vcx.debug_bounds("effort-icon-medium").is_some(),
            "footer draws medium, not a stale effort"
        );
        assert!(
            vcx.debug_bounds("picker-search").is_none(),
            "picker closes after submission"
        );
    }

    fn typed_medium(cx: &mut gpui::TestAppContext, provider: &str, model: &str) {
        let (panel, vcx, commands) = setup(cx, provider, model, Some("high"));
        vcx.simulate_keystrokes("/ e f f o r t space m e d i u m");
        vcx.run_until_parked();
        assert_eq!(
            panel.read_with(vcx, |panel, cx| panel.input.read(cx).content.to_string()),
            "/effort medium"
        );
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["medium"]);
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("medium"));
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.composer_identity().1),
            Some("medium".into())
        );
        assert!(
            vcx.debug_bounds("panel-model-effort").is_some(),
            "footer effort pill is painted"
        );
        assert!(
            vcx.debug_bounds("effort-icon-medium").is_some(),
            "footer draws medium"
        );
    }

    #[gpui::test]
    fn effort_medium_row_click_reaches_opus_55(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, commands) = setup(cx, "claude-oauth", "claude-opus-5-5", Some("high"));
        let pill = vcx.debug_bounds("panel-model-effort").unwrap();
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let medium = vcx
            .debug_bounds("slash-command-row-2")
            .expect("Claude medium row");
        vcx.simulate_click(medium.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["medium"]);
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("medium"));
        assert!(vcx.debug_bounds("effort-icon-medium").is_some());
    }

    #[gpui::test]
    fn model_second_step_accepts_full_effort_command(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, commands) = setup(cx, "openai", "gpt-6.1-sol", Some("high"));
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::RuntimeInfo {
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-6.1-sol".into()),
                    reasoning_effort: Some("high".into()),
                    auth_method: None,
                    account_label: None,
                    routes: vec![jcode_sdk::ModelRouteInfo {
                        model: "claude-opus-5-5".into(),
                        provider: "anthropic".into(),
                        api_method: "claude-oauth".into(),
                        available: true,
                        detail: String::new(),
                        usage: None,
                    }],
                },
                cx,
            );
        });
        vcx.run_until_parked();
        let model = vcx.debug_bounds("panel-model").unwrap();
        vcx.simulate_click(model.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        vcx.simulate_keystrokes("/ m o d e l space c l a u d e - o p u s - 5 - 5 enter");
        vcx.run_until_parked();
        assert!(
            commands.try_iter().next().is_none(),
            "model selection waits for effort"
        );
        vcx.simulate_keystrokes("/ e f f o r t space m e d i u m enter");
        vcx.run_until_parked();
        let sent: Vec<_> = commands.try_iter().collect();
        assert!(sent.iter().any(|command| matches!(command, Command::SetModel { model, .. } if model == "claude-oauth:claude-opus-5-5")), "selected model reaches bridge");
        assert!(sent.iter().any(|command| matches!(command, Command::SessionOperation { operation: SessionOperation::SetEffort(effort), .. } if effort == "medium")), "selected effort reaches bridge");
        assert!(vcx.debug_bounds("effort-icon-medium").is_some());
    }

    /// The configured chords (Alt+Left/Right by default) step one rung at a
    /// time from the focused composer, stop at the ends, and update the pill
    /// immediately without waiting for the runtime.
    #[gpui::test]
    fn fast_effort_keys_step_the_ladder_from_the_composer(cx: &mut gpui::TestAppContext) {
        let keys = crate::effort::EffortKeys::from_config();
        let (Some(up), Some(down)) = (keys.increase.first(), keys.decrease.first()) else {
            return; // Effort keys disabled in this user's config.
        };
        let (panel, vcx, commands) = setup(cx, "openai", "gpt-5.6-sol", Some("high"));

        vcx.simulate_keystrokes(up);
        vcx.run_until_parked();
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("xhigh"));
        vcx.simulate_keystrokes(up);
        vcx.simulate_keystrokes(down);
        vcx.simulate_keystrokes(down);
        vcx.run_until_parked();
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("high"));
        assert_eq!(sent_efforts(&commands), ["xhigh", "max", "xhigh", "high"]);
        assert_eq!(
            panel.read_with(vcx, |panel, cx| panel
                .input
                .read(cx)
                .transient_notice()
                .map(str::to_string)),
            Some("Effort: High".into())
        );
        assert_eq!(
            panel.read_with(vcx, |panel, cx| panel.input.read(cx).content.to_string()),
            "",
            "effort keys must not type or move text in the composer"
        );
    }

    #[gpui::test]
    fn fast_effort_keys_stop_at_the_strongest_level(cx: &mut gpui::TestAppContext) {
        let keys = crate::effort::EffortKeys::from_config();
        let Some(up) = keys.increase.first() else {
            return;
        };
        let (panel, vcx, commands) = setup(cx, "anthropic", "claude-sonnet-4-6", None);
        let top = *panel
            .read_with(vcx, |panel, _| panel.effort_ladder())
            .last()
            .unwrap();
        panel.update(vcx, |panel, _| panel.reasoning_effort = Some(top.into()));
        vcx.simulate_keystrokes(up);
        vcx.run_until_parked();
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some(top));
        assert!(
            sent_efforts(&commands).is_empty(),
            "nothing to send at the top"
        );
        assert!(
            panel
                .read_with(vcx, |panel, cx| panel
                    .input
                    .read(cx)
                    .transient_notice()
                    .map(str::to_string))
                .is_some_and(|notice| notice.contains("already strongest"))
        );
    }

    /// A refused request restores the level the provider is still using, and
    /// an identity broadcast between rapid presses does not snap the pill back.
    #[gpui::test]
    fn a_refusal_restores_the_previous_effort(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, commands) = setup(cx, "openai", "gpt-5.6-sol", Some("medium"));
        panel.update(vcx, |panel, cx| {
            assert!(panel.request_effort("high", cx));
            assert!(panel.request_effort("xhigh", cx));
            // The first change's broadcast arrives while xhigh is in flight.
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6-sol".into()),
                    reasoning_effort: Some("high".into()),
                    auth_method: None,
                    account_label: None,
                },
                cx,
            );
            assert_eq!(panel.reasoning_effort.as_deref(), Some("xhigh"));
            panel.effort_settled("high", None, cx);
            panel.effort_settled("xhigh", Some("not supported"), cx);
            assert_eq!(panel.reasoning_effort.as_deref(), Some("high"));
            assert!(matches!(panel.items.last(), Some(Item::Error(error)) if error.contains("not supported")));
            // Settled: later identity is authoritative again.
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6-sol".into()),
                    reasoning_effort: None,
                    auth_method: None,
                    account_label: None,
                },
                cx,
            );
            assert_eq!(panel.reasoning_effort, None);
        });
        assert_eq!(sent_efforts(&commands), ["high", "xhigh"]);
    }

    /// `/effort` offers and accepts only the serving model's levels.
    #[gpui::test]
    fn slash_effort_follows_the_serving_model(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, commands) = setup(cx, "anthropic", "claude-sonnet-4-6", Some("high"));
        vcx.simulate_input("/effort minimal");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(
            sent_efforts(&commands).is_empty(),
            "Claude has no minimal level"
        );
        assert!(panel.read_with(vcx, |panel, _| matches!(
            panel.items.last(),
            Some(Item::Error(error)) if error.contains("low|medium|high")
        )));

        panel.update(vcx, |panel, cx| {
            panel
                .input
                .update(cx, |input, cx| input.set_content(String::new(), cx))
        });
        vcx.simulate_input("/effort Low");
        vcx.simulate_keystrokes("enter");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["low"]);
        assert_eq!(effort_of(&panel, vcx).as_deref(), Some("low"));
    }

    /// The `/effort` picker rows (None … Max, Swarm) each lead with a drawn
    /// level mark, never a `◔◑◕●` text glyph: the row label is the name alone
    /// and the shape is a canvas element beside it.
    #[gpui::test]
    fn effort_picker_rows_draw_their_level_marks(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, _commands) = setup(cx, "openai", "gpt-5.6-sol", Some("high"));
        vcx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.toggle_effort_picker(window, cx))
        });
        vcx.run_until_parked();
        let rows = panel.read_with(vcx, |panel, _| {
            panel
                .effort_ladder()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
        assert!(rows.len() >= 5, "{rows:?}");
        let mut drawn = 0;
        for (index, level) in rows.iter().enumerate() {
            let label = crate::effort::spoken(level);
            assert!(
                label.chars().all(|c| c.is_ascii_alphanumeric() || c == ' '),
                "{level} row label carries a text glyph: {label:?}"
            );
            let selector: &'static str =
                Box::leak(format!("effort-row-icon-{index}").into_boxed_str());
            if vcx.debug_bounds(selector).is_some() {
                drawn += 1;
            }
            assert!(
                crate::effort_icon::shape(level).is_some(),
                "{level} has a shape"
            );
        }
        assert!(drawn > 0, "visible rows draw their marks");
    }

    /// The pill menu opens on the current level, so Enter keeps it.
    #[gpui::test]
    fn effort_menu_opens_on_the_current_level(cx: &mut gpui::TestAppContext) {
        let (panel, vcx, commands) = setup(cx, "openai", "gpt-5.6-sol", Some("xhigh"));
        vcx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.toggle_effort_picker(window, cx))
        });
        vcx.run_until_parked();
        vcx.simulate_keystrokes("down enter");
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["max"]);
        assert_eq!(
            panel.read_with(vcx, |panel, cx| panel.input.read(cx).content.to_string()),
            "",
            "the menu closes after choosing"
        );
    }

    #[gpui::test]
    fn effort_picker_keeps_draft_while_searching_selecting_and_cancelling(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx, commands) = setup(cx, "openai", "gpt-5.6-sol", Some("xhigh"));
        vcx.simulate_input("valuable draft");
        vcx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_effort_picker(window, cx)));
        vcx.run_until_parked();
        assert_eq!(
            panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
            "valuable draft"
        );
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert_eq!(
            panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
            "valuable draft"
        );
        vcx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_effort_picker(window, cx)));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("down enter");
        vcx.run_until_parked();
        assert_eq!(sent_efforts(&commands), ["max"]);
        assert_eq!(
            panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
            "valuable draft"
        );
    }
}
