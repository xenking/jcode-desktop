//! Workspace-level switches: overview, hints, showcase, theme and sidebar.
use super::*;

impl Workspace {
    pub(super) fn toggle_overview(&mut self, _: &ToggleOverview, window: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("O", "Toggle overview", "overview", cx);
        self.learned("overview", cx);
        self.overview = !self.overview;
        self.map_gesture.reset_view();
        if self.overview {
            self.map_gesture
                .focus_session(self.active_position_in_row(), self.active_row);
        }
        self.overview_progress
            .set(if self.overview { 1.0 } else { 0.0 }, Instant::now());
        self.hints_overlay = false;
        self.hints_progress.set(0.0, Instant::now());
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_hints(&mut self, _: &ToggleHints, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("/", "Show all shortcuts", "help", cx);
        self.hints_overlay = !self.hints_overlay;
        self.hints_progress
            .set(if self.hints_overlay { 1.0 } else { 0.0 }, Instant::now());
        cx.notify();
    }

    pub(super) fn toggle_showcase(&mut self, _: &ToggleShowcase, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_mode = !self.showcase_mode;
        self.showcase_cue = self.showcase_mode.then(|| ShowcaseCue {
            started_at: Some(Instant::now()),
            shortcut: if cfg!(target_os = "macos") {
                "Cmd + Shift + S".to_owned()
            } else {
                "Super + Shift + S".to_owned()
            },
            action: "Showcase mode on",
            tutorial_group: "",
        });
        self.schedule_showcase_expiry(cx);
        cx.notify();
    }

    pub(super) fn select_theme(&mut self, preset: ThemePreset, cx: &mut Context<Self>) {
        if preset == Theme::active_preset() {
            cx.notify();
            return;
        }
        Theme::select(preset);
        if let Err(error) = crate::config::persist_theme(preset.id()) {
            eprintln!("failed to persist desktop theme: {error}");
        }
        cx.notify();
    }

    pub(super) fn cycle_theme(&mut self, _: &CycleTheme, _: &mut Window, cx: &mut Context<Self>) {
        self.select_theme(Theme::active_preset().next(), cx);
    }

    pub(super) fn new_help_session(&mut self, _: &NewHelpSession, _: &mut Window, cx: &mut Context<Self>) {
        self.hints_overlay = false;
        self.hints_progress.set(0.0, Instant::now());
        self.open_local_draft_kind(self.default_working_dir(), true, cx);
        cx.notify();
    }

    /// Toggle the visible sidebar surface, not the hidden column preference.
    /// Narrow windows use a drawer and keep sessions at full width. Wide
    /// windows toggle the persistent column without changing panel slots.
    pub(super) fn toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_compact(f32::from(window.viewport_size().width)) {
            self.compact_sidebar_open = !self.compact_sidebar_open;
            if self.compact_sidebar_open {
                self.show_sidebar = true;
            }
        } else {
            self.compact_sidebar_open = false;
            self.show_sidebar = !self.show_sidebar;
        }
        cx.notify();
    }
}
