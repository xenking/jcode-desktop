//! Workspace: four niri-inspired horizontal strips of session panels with a
//! smooth camera and a zoomed-out overview.
//!
//! Panels live on one of four infinite horizontal strips. Focus moves
//! left/right within a strip and up/down between strips.

#[cfg(test)]
#[path = "workspace_control_tests.rs"]
mod control_tests;

#[cfg(test)]
#[path = "workspace_file_editor_tests.rs"]
mod file_editor_tests;

#[cfg(test)]
#[path = "workspace_terminal_tests.rs"]
mod terminal_tests;

#[path = "workspace_control.rs"]
mod control;

#[path = "workspace_commands.rs"]
mod commands;

#[path = "workspace_account_sign_in.rs"]
mod account_sign_in;

#[path = "workspace_applets.rs"]
pub(crate) mod applets;

#[path = "workspace_applet_surfaces.rs"]
mod applet_surfaces;
#[path = "workspace_side_panel.rs"]
mod side_panel;

#[path = "workspace_resume.rs"]
pub(crate) mod resume;

#[path = "workspace_menu.rs"]
mod menu;
#[path = "workspace_single_panel.rs"]
mod single_panel;

#[path = "workspace_global_voice.rs"]
mod global_voice;

#[path = "workspace_panel_window.rs"]
pub(crate) mod panel_window;

#[path = "workspace_preview.rs"]
mod preview;

#[path = "workspace_changelog.rs"]
mod changelog_panel;

#[path = "workspace_publish.rs"]
mod publish_panel;

#[path = "workspace_switch_directory.rs"]
mod switch_directory;

#[path = "workspace_responsive.rs"]
mod responsive;
#[path = "workspace_files.rs"]
pub(crate) mod files;

#[path = "workspace_notifications.rs"]
mod notifications;

#[path = "workspace_change_review.rs"]
pub(crate) mod change_review;

#[path = "sidebar_account_menu.rs"]
mod account_menu;
#[path = "sidebar_account.rs"]
mod sidebar_account;
#[path = "sidebar_routing.rs"]
mod sidebar_routing;
#[path = "sidebar_collections.rs"]
mod sidebar_collections;
#[path = "sidebar_edits.rs"]
mod sidebar_edits;
#[path = "sidebar_gesture.rs"]
mod sidebar_gesture;
#[path = "sidebar_header.rs"]
mod sidebar_header;
#[cfg(test)]
#[path = "sidebar_header_tests.rs"]
mod sidebar_header_tests;
#[path = "workspace_sidebar_metadata.rs"]
mod sidebar_metadata;
#[path = "sidebar_projects.rs"]
mod sidebar_projects;
#[path = "sidebar_selection.rs"]
mod sidebar_selection;
#[path = "sidebar_session_catalog.rs"]
mod sidebar_session_catalog;
#[path = "sidebar_swarm.rs"]
mod sidebar_swarm;
#[path = "sidebar_worktrees.rs"]
mod sidebar_worktrees;
#[path = "workspace_stream_fixture.rs"]
mod workspace_stream_fixture;
use sidebar_metadata::SidebarPanelMetadata;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, ScrollHandle, Window, actions, div, prelude::*,
    px, relative,
};
use jcode_desktop_api::HostHandle;
use serde::{Deserialize, Serialize};

use crate::accounts;
use crate::harness::{self, Bridge, Command, Update};
use crate::input::{PromptInput, PromptInputSnapshot};
use crate::learning;
use crate::modal_layer::{HiddenWithoutFocus, ModalLayer, ModalView};
use crate::panel::{Panel, PanelSnapshot};
use crate::panel_item::PanelItem;
use crate::panel_item::changelog_panel::ChangelogPanel;
use crate::panel_item::file_panel::FilePanel;
use crate::performance::{
    ActionCapture, GpuiSnapshot as GpuiPerformanceSnapshot, Health as PerformanceHealth,
    Profile as PerformanceProfile,
};
use crate::terminal::TerminalPanel;
use crate::theme::{Ink, Palette, Theme, ThemePreset};
use crate::transition::{self, AnimatedValue, Transition};
use crate::updates;

#[path = "workspace_recovery.rs"]
pub(crate) mod recovery;
#[path = "workspace_restart.rs"]
pub(crate) mod restart;
#[path = "workspace_housekeeping.rs"]
mod housekeeping;

#[cfg(test)]
#[path = "closing_navigation_tests.rs"]
mod closing_navigation_tests;
#[cfg(test)]
#[path = "fps_header_tests.rs"]
mod fps_header_tests;
#[cfg(test)]
#[path = "hidden_animation_tests.rs"]
mod hidden_animation_tests;
#[path = "workspace_map_gesture.rs"]
mod map_gesture;
#[path = "workspace_map_motion.rs"]
mod map_motion;
#[cfg(test)]
#[path = "workspace_nav_profile_tests.rs"]
mod nav_profile_tests;
#[cfg(test)]
#[path = "navigation_map_tests.rs"]
mod navigation_map_tests;
#[path = "navigation_state.rs"]
mod navigation_state;
#[path = "workspace_openrig.rs"]
mod openrig;
#[path = "workspace_fleet_session.rs"]
mod fleet_session;
#[cfg(test)]
#[path = "panel_cache_tests.rs"]
pub(crate) mod panel_cache_tests;
#[cfg(test)]
#[path = "workspace_remote_tests.rs"]
mod remote_tests;
#[path = "workspace_remotes.rs"]
mod remotes;
#[path = "workspace_scroll_snap.rs"]
mod scroll_snap;
#[cfg(test)]
#[path = "super_action_behavior_tests.rs"]
mod super_action_behavior_tests;
#[path = "workspace_voice.rs"]
mod voice;
#[path = "window_navigation.rs"]
mod window_navigation;
#[cfg(test)]
#[path = "window_navigation_tests.rs"]
mod window_navigation_tests;
#[cfg(test)]
#[path = "workspace_lone_panel_tests.rs"]
mod workspace_lone_panel_tests;

#[path = "workspace_login_rows.rs"]
mod login_rows;
#[path = "workspace_usage_reset.rs"]
mod usage_reset;

#[path = "workspace_rename.rs"]
mod rename;

#[path = "workspace_pin.rs"]
mod pin;

#[path = "workspace_default_directory.rs"]
mod default_directory;
#[path = "workspace_folder_picker.rs"]
mod folder_picker;
use folder_picker::FolderPicker;
#[cfg(test)]
#[path = "workspace_default_directory_tests.rs"]
mod default_directory_tests;
#[cfg(test)]
#[path = "workspace_modal_tests.rs"]
mod modal_tests;

#[cfg(test)]
#[path = "workspace_resume_search_tests.rs"]
mod resume_search_tests;

// Actions that panels dispatch to the workspace live in a leaf module so the
// panel layer does not depend on this one.
pub(crate) use crate::ui_actions::{
    ClosePanel, GlobalSearch, OpenAccounts, OpenAppletShowcase, OpenChangelog, OpenResume,
    PublishDesktop, SwitchDirectory, ToggleOnboardingSimulator,
};

#[derive(Clone, PartialEq, gpui::Action)]
#[action(no_json)]
pub(crate) struct OpenModelPicker {
    pub source: gpui::EntityId,
}

actions!(
    workspace,
    [
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        FocusFirst,
        FocusLast,
        FocusPrevious,
        MovePanelLeft,
        MovePanelRight,
        MovePanelUp,
        MovePanelDown,
        MovePanelToFirst,
        MovePanelToLast,
        NewPanel,
        NewPanelInPinnedDirectory,
        ForkPanel,
        RenameSession,
        NewTerminal,
        OpenGmail,
        OpenTodoist,
        OpenOrchestration,
        OpenOpenrig,
        NewUnfinishedWork,
        OpenFolder,
        ToggleOverview,
        ToggleHints,
        ToggleShowcase,
        ToggleSidebar,
        ToggleVoice,
        BeginVoiceHold,
        EndVoiceHold,
        BeginGlobalVoiceHold,
        EndGlobalVoiceHold,
        CycleTheme,
        NewHelpSession,
        CycleWidth,
        MaximizeWidth,
        WidthPreset1,
        WidthPreset2,
        WidthPreset3,
        WidthPreset4,
        Quit,
    ]
);

/// Spatial transitions settle in 150 ms. Cubic easing keeps motion visible
/// across the available frames instead of concentrating it at the start.
#[allow(dead_code)]
const CAMERA_DURATION: Duration = transition::STANDARD_DURATION;
/// The bundled applet behind Super+Shift+G and the sidebar inbox entry.
const GMAIL_APPLET: &str = "gmail";
/// A tiny amount of presentation smoothing removes the one-frame stepping
/// caused by touchpad events arriving between compositor frames without making
/// the canvas feel detached from the fingers.
const TOUCH_PAN_DURATION: Duration = Duration::from_millis(42);
/// niri `layout { gaps 0 }`: columns sit flush against each other.
const GAP: f32 = 0.0;
/// niri `layout { struts { ... 0.58 } }`, the outer gap around the strip.
const STRUT: f32 = 0.58;
/// Folder surfaces sit flush against the window's bottom edge.
const STRIP_PADDING_Y: f32 = 0.0;
/// A tighter header keeps floating tabs close to the top without crowding the footer.
const STRIP_PADDING_TOP: f32 = 6.0;
/// Reserve a dedicated top row for the live-session folder tabs.
const FOLDER_CONTENT_INSET: f32 = 32.0;
/// Shared bottom metadata stays outside transcript and composer hit targets.
/// The session sheet sits flush against the sidebar and the window's right edge.
const FOLDER_CONNECTOR_WIDTH: f32 = 0.0;
const FOLDER_RIGHT_MARGIN: f32 = 0.0;

#[path = "folder_surface.rs"]
mod folder_surface;

#[path = "live_tabs.rs"]
mod live_tabs;
#[path = "workspace_version.rs"]
mod version;

#[path = "sidebar_roller.rs"]
mod sidebar_roller;
#[path = "sidebar_search_button.rs"]
mod sidebar_search_button;

#[cfg(test)]
#[path = "panel_surface_tests.rs"]
mod panel_surface_tests;

#[cfg(test)]
#[path = "hover_scroll_tests.rs"]
mod hover_scroll_tests;

#[cfg(test)]
#[path = "focus_follow_tests.rs"]
mod focus_follow_tests;
#[path = "workspace_pull.rs"]
mod workspace_pull;
#[cfg(test)]
#[path = "workspace_scroll_tests.rs"]
mod workspace_scroll_tests;

#[cfg(test)]
#[path = "workspace_orchestration_tests.rs"]
mod workspace_orchestration_tests;

#[path = "tutorial.rs"]
mod tutorial;

#[path = "onboarding_simulator.rs"]
mod onboarding_simulator;

#[path = "workspace_beta_notice.rs"]
mod beta_notice;

#[cfg(test)]
#[path = "tutorial_geometry_tests.rs"]
mod tutorial_geometry_tests;

#[cfg(test)]
#[path = "tutorial_minimalism_tests.rs"]
mod tutorial_minimalism_tests;

const STRIP_COUNT: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShowcaseCue {
    shortcut: String,
    action: &'static str,
    tutorial_group: &'static str,
    /// None keeps the screenshot design fixture stationary.
    started_at: Option<Instant>,
}

/// niri `window-rule { geometry-corner-radius 6 }`.
const CORNER_RADIUS: f32 = 6.0;
/// niri `preset-column-widths`: Alt+R cycles through these in order.
const PRESET_WIDTHS: [f32; 3] = [0.25, 0.5, 0.75];
/// niri `default-column-width { proportion 0.5; }`.
const DEFAULT_WIDTH: f32 = 0.5;
const HELP_SESSION_PROMPT: &str = r#"Act as the in-app Jcode guide. Use the bundled Jcode documentation before answering questions about Jcode features or behavior.

The jcode-desktop shortcuts are:
- Super+H/J/K/L: navigate panels
- Super+Shift+H/J/K/L: move panels
- Super+Enter: open a session to the right
- Ctrl+O: choose a folder and open a session there
- Super+Tab: return to the previous panel
- Super+R: cycle panel width
- Super+F: maximize or restore panel width
- Super+O: open the overview
- Super+/ or F1: toggle the hints overlay
- F2: rename the active session (also available from the Rename button)
- Super+Shift+S: toggle showcase mode for on-screen workspace motions
- Super+Shift+T: cycle the desktop theme
- Super+Shift+/: open this documentation-aware help session

Composer shortcuts ported from the TUI:
- Up/Down or Ctrl+K/J: recall older/newer prompts
- Ctrl/Alt+Left/Right or Alt+B/F: move by word
- Ctrl+W, Ctrl/Alt/Super+Backspace, Alt+D: delete by word
- Ctrl+U: delete to the start; Ctrl+E: move to the end
- Ctrl/Cmd+Z and Ctrl+Shift+Z: undo and redo
- Escape: clear the draft

Start with a concise orientation, then invite me to ask how to use Jcode."#;
const SIDEBAR_WIDTH: f32 = 224.0;
// With no connector gap the gutter stays inside the sidebar in every layout.
const SIDEBAR_SCROLLBAR_OUTSET: f32 = 0.0;
const ACCOUNT_ROW_HEIGHT: f32 = 60.0;

/// The 3 px usage bar under an account or key window: `fraction` of the track
/// in `color`. Every accounts-sidebar bar goes through here.
fn usage_bar(fraction: f32, color: gpui::Rgba) -> gpui::Div {
    div()
        .w_full()
        .h(px(3.0))
        .rounded_full()
        .overflow_hidden()
        .bg(Theme::global().INLINE_CODE_BG)
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(color),
        )
}
/// Height of the macOS titlebar the window draws through. The window uses a
/// transparent system titlebar, so the app's own chrome has to leave this much
/// room at the top or it renders underneath the traffic lights.
const TITLEBAR_HEIGHT: f32 = 40.0;
/// The macOS traffic lights end at 76 points. Leave 8 points before app chrome.
const MACOS_TRAFFIC_LIGHTS_WIDTH: f32 = 84.0;

fn content_top_inset(show_sidebar: bool, fullscreen: bool) -> f32 {
    // Fullscreen macOS windows have no titlebar and the traffic lights are
    // hidden (they only float in on hover, above our content), so reserving
    // room for them would leave a dead strip across the top.
    if cfg!(target_os = "macos") && !show_sidebar && !fullscreen {
        TITLEBAR_HEIGHT
    } else {
        0.0
    }
}

fn sidebar_header_left_padding(fullscreen: bool) -> f32 {
    // Same reasoning as `content_top_inset`: in fullscreen the traffic lights
    // are not in the header row, so keep the normal navigation alignment.
    if cfg!(target_os = "macos") && !fullscreen {
        MACOS_TRAFFIC_LIGHTS_WIDTH
    } else {
        12.0
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
enum SidebarView {
    #[default]
    #[serde(alias = "Machines")]
    Sessions,
    Learn,
    Files,
    Accounts,
    Theme,
    Settings,
}

// Minimap: a compact card in the top right that maps every strip to
// scale, preserving the canvas aspect ratio so panels taller than wide on
// screen stay taller than wide on the map.
const MINIMAP_LABEL_WIDTH: f32 = 22.0;
const MINIMAP_WIDTH: f32 = 112.0 + MINIMAP_LABEL_WIDTH;
const MINIMAP_HEIGHT: f32 = 96.0;
const MINIMAP_PADDING: f32 = 5.0;
const MINIMAP_ROW_GAP: f32 = 3.0;
const MINIMAP_TOP: f32 = if cfg!(target_os = "macos") {
    STRIP_PADDING_TOP + FOLDER_CONTENT_INSET + 6.0
} else if crate::window_caption::enabled() {
    // Below the Windows caption buttons, which own the top-right corner.
    crate::window_caption::HEIGHT + 6.0
} else {
    8.0
};
const MINIMAP_RIGHT: f32 = 12.0;
/// The update chip sits above the workspace bar in the bottom-right corner,
/// out of the reading path but always in view.
const UPDATE_CHIP_BOTTOM: f32 = 44.0;
const PAIRED_UPDATE_ROW_HEIGHT: f32 = 32.0;
/// Update the countdown once per second, never at display refresh rate.
const COACH_EXPIRY_WAKE: Duration = Duration::from_secs(1);
const SHOWCASE_DURATION: Duration = Duration::from_millis(1800);
/// The hands-on curriculum in the opt-in Learn tab. Practiced skills persist
/// across launches, and the completed tutorial remains a shortcut reference.
const ONBOARDING_SKILLS: &[&str] = &[
    "focus_left_right",
    "focus_up_down",
    "new_panel",
    "close_panel",
    "move_panel",
    "move_panel_strip",
    "width_presets",
    "cycle_width",
    "overview",
];
/// Rows split the square's inner height evenly, one per strip.
const MINIMAP_ROW_HEIGHT: f32 =
    (MINIMAP_HEIGHT - MINIMAP_PADDING * 2.0 - MINIMAP_ROW_GAP * (STRIP_COUNT as f32 - 1.0))
        / STRIP_COUNT as f32;
/// Vertical inset between a panel rectangle and its track edge.
const MINIMAP_PANEL_INSET: f32 = 1.5;
/// The gesture reticle: how long it stays fully lit after the last touchpad
/// scroll delta, and how long the fade-out takes once the fingers lift.
const GESTURE_HOLD: Duration = Duration::from_millis(150);
const GESTURE_FADE: Duration = Duration::from_millis(300);
const GESTURE_RETICLE_SIZE: f32 = 44.0;
const MINIMAP_GESTURE_DOT: f32 = 7.0;
/// Travel before a touchpad gesture locks to its dominant axis. Below this,
/// horizontal deltas pan and vertical deltas scroll, like before.
const AXIS_LOCK: f32 = 12.0;
/// Vertical travel, after a horizontal lock, that breaks the sticky axis and
/// hops to the neighbouring strip. A gesture can hop only once, so its
/// remaining travel and momentum cannot skip past the destination.
const STRIP_BREAK: f32 = 260.0;
/// A pause this long between deltas ends the gesture, since not every
/// platform reliably delivers an Ended touch phase.
const GESTURE_RESET: Duration = Duration::from_millis(250);
/// Fraction of the vertical pull the reticle follows while the sticky axis
/// resists, so the rubber band is visible before it snaps.
const PULL_RESISTANCE: f32 = 0.22;

/// Which axis a touchpad gesture has committed to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum GestureAxis {
    #[default]
    Undecided,
    Horizontal,
    Vertical,
}

/// The sticky axis lock for two-finger strip gestures. A gesture that starts
/// horizontally owns the strip: it pans, and once enough vertical travel
/// accumulates it breaks the stickiness and hops strips. A gesture that
/// starts vertically belongs to whatever is under the pointer and never pans.
#[derive(Clone, Copy, Debug, Default)]
struct StripGesture {
    axis: GestureAxis,
    travel_x: f32,
    travel_y: f32,
    /// Vertical pull accumulated while horizontally locked. Positive pulls
    /// toward the strip below (natural: fingers up reveal what is beneath).
    pull: f32,
    /// Remains latched through Ended and momentum until a fresh gesture.
    switched: bool,
}

/// Where one scroll delta goes, decided by the sticky axis lock.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Routed {
    /// The event belongs to whatever is under the pointer (a transcript).
    ToPanel,
    /// The strip takes it: pan by `dx`, hop `switch` strips, and when
    /// `exclusive` the event must not also reach the transcript.
    Strip {
        dx: f32,
        switch: i8,
        exclusive: bool,
    },
}

impl StripGesture {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn begin_phase(&mut self, phase: gpui::TouchPhase) -> bool {
        use gpui::TouchPhase::{Cancelled, Ended, Started};
        if phase == Started {
            self.reset();
        }
        if matches!(phase, Ended | Cancelled) {
            // Some platforms send momentum as Moved after fingers lift.
            // Clear the axis/pull, but do not re-arm a completed swipe.
            let switched = self.switched || phase == Cancelled;
            self.reset();
            self.switched = switched;
            return false;
        }
        true
    }

    fn pull_vertical(&mut self, dy: f32) -> i8 {
        if self.switched {
            return 0;
        }
        self.pull -= dy;
        if self.pull.abs() < STRIP_BREAK {
            return 0;
        }
        let direction = if self.pull > 0.0 { 1 } else { -1 };
        self.pull = 0.0;
        self.switched = true;
        direction
    }

    /// Empty panels and empty rows share the same latch as axis breakouts.
    fn feed_vertical(&mut self, dy: f32, phase: gpui::TouchPhase) -> i8 {
        if !self.begin_phase(phase) {
            return 0;
        }
        if self.axis != GestureAxis::Vertical {
            self.pull = 0.0;
            self.axis = GestureAxis::Vertical;
        }
        self.pull_vertical(dy)
    }

    /// Advance the state machine by one precise (touchpad) delta.
    fn feed(&mut self, dx: f32, dy: f32, phase: gpui::TouchPhase) -> Routed {
        let owned_by_strip = self.switched || self.axis == GestureAxis::Horizontal;
        if !self.begin_phase(phase) {
            if !owned_by_strip {
                return Routed::ToPanel;
            }
            return Routed::Strip {
                dx: 0.0,
                switch: 0,
                exclusive: true,
            };
        }
        if self.switched {
            // Do not leak the tail of a workspace swipe into the destination
            // transcript. Horizontal panning can still continue normally.
            return Routed::Strip {
                dx,
                switch: 0,
                exclusive: true,
            };
        }
        match self.axis {
            GestureAxis::Undecided => {
                self.travel_x += dx.abs();
                self.travel_y += dy.abs();
                if self.travel_x >= AXIS_LOCK || self.travel_y >= AXIS_LOCK {
                    self.axis = if self.travel_x >= self.travel_y {
                        GestureAxis::Horizontal
                    } else {
                        GestureAxis::Vertical
                    };
                }
                match self.axis {
                    GestureAxis::Vertical => Routed::ToPanel,
                    // Pan horizontal motion immediately, but keep the event
                    // shared until the lock resolves so the first few small
                    // vertical pixels still reach the transcript.
                    axis => Routed::Strip {
                        dx,
                        switch: 0,
                        exclusive: axis == GestureAxis::Horizontal,
                    },
                }
            }
            GestureAxis::Vertical => Routed::ToPanel,
            GestureAxis::Horizontal => {
                let switch = self.pull_vertical(dy);
                Routed::Strip {
                    dx,
                    switch,
                    exclusive: true,
                }
            }
        }
    }
}

struct Slot {
    panel: PanelItem,
    row: usize,
    /// Width as a fraction of the viewport (0.25, 0.5, 0.75, 1.0).
    width_fraction: f32,
    /// Rendered width, retargeted when the configured width changes.
    animated_width: AnimatedValue,
    /// Signed progress from the panel's former horizontal position to its new
    /// one. The distance uses the swapped neighbour's width at render time.
    order_offset: AnimatedValue,
    order_distance_fraction: f32,
    /// Visibility during dismissal. Closed panels remain mounted until this
    /// reaches zero so their exit can actually be painted.
    close_progress: AnimatedValue,
    closing: bool,
    /// Width to restore when un-maximizing (niri `maximize-column` toggle).
    restore_fraction: Option<f32>,
}

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// A slot restored from a snapshot, at its saved place and width.
fn restored_slot(
    panel: PanelItem,
    row: usize,
    width_fraction: f32,
    restore_fraction: Option<f32>,
) -> Slot {
    Slot {
        panel,
        row,
        width_fraction,
        animated_width: AnimatedValue::new(
            width_fraction,
            transition::policy(Transition::PanelWidth).duration,
        ),
        order_offset: AnimatedValue::new(0.0, transition::policy(Transition::PanelOrder).duration),
        order_distance_fraction: width_fraction,
        close_progress: AnimatedValue::new(
            1.0,
            transition::policy(Transition::PanelClose).duration,
        ),
        closing: false,
        restore_fraction,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct SlotSnapshot {
    panel: PanelSnapshot,
    row: usize,
    width_fraction: f32,
    restore_fraction: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
enum FocusSnapshot {
    Panel(usize),
    FolderSearch,
    #[default]
    Workspace,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    format_version: u32,
    #[serde(default)]
    resume: resume::Snapshot,
    #[serde(default)]
    side_panel_snapshots: HashMap<String, side_panel::SidePanelRoutingState>,
    #[serde(default)]
    layout_mode: crate::config::LayoutMode,
    #[serde(default)]
    recent_accounts: Vec<String>,
    #[serde(default)]
    sidebar_closed_sessions: HashSet<String>,
    #[serde(default)]
    sidebar_view: SidebarView,
    #[serde(default)]
    session_catalog: sidebar_session_catalog::Preferences,
    #[serde(default)]
    tutorial_page: usize,
    slots: Vec<SlotSnapshot>,
    active: usize,
    active_row: usize,
    row_focus: [Option<usize>; STRIP_COUNT],
    previous: Option<usize>,
    #[serde(default)]
    last_voice_chat: Option<usize>,
    camera_x: [f32; STRIP_COUNT],
    camera_target: [f32; STRIP_COUNT],
    overview: bool,
    hints_overlay: bool,
    #[serde(default)]
    folder_picker_sets_default: bool,
    folder_picker_dir: Option<PathBuf>,
    folder_picker_error: Option<String>,
    folder_search: Option<PromptInputSnapshot>,
    focus: FocusSnapshot,
    /// The window's own launch, kept across Ctrl+R in a shared host. Older
    /// snapshots omit it and fall back to the process launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    launch: Option<LaunchSnapshot>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
struct LaunchSnapshot {
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl From<&jcode_desktop_api::WindowLaunch> for LaunchSnapshot {
    fn from(launch: &jcode_desktop_api::WindowLaunch) -> Self {
        Self {
            args: launch
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: launch.env.clone(),
        }
    }
}

impl From<LaunchSnapshot> for jcode_desktop_api::WindowLaunch {
    fn from(launch: LaunchSnapshot) -> Self {
        Self::from_parts(launch.args, launch.env)
    }
}

impl WorkspaceSnapshot {
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(Into::into)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let snapshot: Self = serde_json::from_slice(bytes)?;
        anyhow::ensure!(
            snapshot.format_version == SNAPSHOT_FORMAT_VERSION,
            "unsupported workspace snapshot format {}",
            snapshot.format_version
        );
        anyhow::ensure!(
            snapshot.active_row < STRIP_COUNT,
            "active row is out of range"
        );
        anyhow::ensure!(
            snapshot.slots.iter().all(|slot| {
                slot.row < STRIP_COUNT
                    && slot.width_fraction.is_finite()
                    && (0.1..=1.0).contains(&slot.width_fraction)
                    && slot
                        .restore_fraction
                        .is_none_or(|width| width.is_finite() && (0.1..=1.0).contains(&width))
            }),
            "snapshot contains an invalid panel layout"
        );
        anyhow::ensure!(
            snapshot.slots.is_empty() || snapshot.active < snapshot.slots.len(),
            "active panel is out of range"
        );
        Ok(snapshot)
    }
}

/// Local sessions some jcode process, normally the shared daemon, is running a
/// turn for right now. Reads only the small streaming-marker directory.
fn daemon_running_sessions() -> HashSet<String> {
    jcode_base::session::streaming_session_ids()
        .into_iter()
        .collect()
}

pub struct Workspace {
    /// Last window background applied for the active theme (blur for Glass).
    window_background: Option<gpui::WindowBackgroundAppearance>,
    global_voice: global_voice::State,
    voice_key: voice::CopilotLatch,
    last_voice_chat: Option<gpui::EntityId>,
    _voice_activation: Option<gpui::Subscription>,
    #[cfg(unix)]
    desktop_control: Option<crate::desktop_control::Server>,
    #[cfg(unix)]
    desktop_control_task: Option<gpui::Task<()>>,
    control_drafts: HashMap<gpui::EntityId, (String, u64)>,
    preview_control: Option<crate::preview_control::Server>,
    preview_task: Option<gpui::Task<()>>,
    side_panel_snapshots: HashMap<String, side_panel::SidePanelRoutingState>,
    /// Latest applet toast and when it arrived.
    applet_toast: Option<(String, Instant)>,
    /// Applet ids whose consent prompt the user dismissed for this run.
    applet_consent_dismissed: HashSet<String>,
    /// Text selection scope for sidebar and overlay applet cards.
    applet_text_selection: Option<Entity<crate::text_selection::TextSelection>>,
    /// Sidebar applet section, rendered with window access before the sidebar.
    sidebar_applets: Option<gpui::AnyElement>,
    sidebar_chrome: Option<Entity<SidebarChrome>>,
    sidebar_panel_metadata: Option<Entity<HashMap<gpui::EntityId, SidebarPanelMetadata>>>,
    panel_metadata_observers: HashMap<gpui::EntityId, gpui::Subscription>,
    bridge: Bridge,
    remotes: remotes::Machines,
    host: HostHandle,
    /// Launch-only presentation, independent of persisted workspace preferences.
    single_panel: bool,
    /// Arguments and forwarded environment of the launch that opened this
    /// window. A shared single-panel host has one per window.
    launch: jcode_desktop_api::WindowLaunch,
    resume: Option<resume::State>,
    /// Full-height global search over every stored session. Transient by
    /// design: relaunching the app lands on the workspace, not on a query.
    show_sidebar: bool,
    sidebar_custom_width: f32,
    sidebar_resizing: bool,
    // Launch-only chrome. Reload snapshots must never re-open the notice.
    account_sign_in: account_sign_in::State,
    account_menu: account_menu::State,
    compact_sidebar_open: bool,
    compact_sidebar_motion: responsive::VisibilityMotion,
    last_canvas_width: Option<f32>,
    // Temporarily hidden. Keep the renderer available for re-enabling later.
    show_minimap: bool,
    layout_mode: crate::config::LayoutMode,
    folder_frame: folder_surface::SharedFrame,
    sidebar_view: SidebarView,
    session_catalog: sidebar_session_catalog::Preferences,
    worktrees: sidebar_worktrees::State,
    /// User-created collections of sessions (issue #7).
    collections: sidebar_collections::State,
    /// Drafts started inside a collection: (draft request id, collection id).
    pending_collection_drafts: Vec<(String, String)>,
    tutorial_page: usize,
    onboarding_launch: onboarding_simulator::LaunchState,
    expanded_directories: HashSet<PathBuf>,
    file_tree: std::cell::RefCell<FileTree>,
    files_pane: files::FilesPane,
    /// Truncated workspace text the user opened: a worker label, a coach
    /// hint. Runtime-only UI state, never part of the saved layout.
    expanded_lines: HashSet<String>,
    slots: Vec<Slot>,
    active: usize,
    active_row: usize,
    /// Last focused panel on each strip, so vertical navigation restores the
    /// place the user left instead of choosing by column position.
    row_focus: [Option<gpui::EntityId>; STRIP_COUNT],
    /// Row being animated out and progress of the incoming row.
    outgoing_row: Option<usize>,
    row_progress: AnimatedValue,
    workspace_pull: workspace_pull::PullPreview,
    /// Vertical origin of the current row trip, in rows.
    row_origin: f32,
    /// Previously focused panel, for niri's `focus-window-previous`.
    previous: Option<gpui::EntityId>,
    /// Each strip retains its own horizontal camera position.
    camera_x: [f32; STRIP_COUNT],
    camera_target: [f32; STRIP_COUNT],
    /// Where the current camera animation started, and when.
    camera_from: [f32; STRIP_COUNT],
    camera_started: [Option<Instant>; STRIP_COUNT],
    camera_touch_pan: [bool; STRIP_COUNT],
    /// Set when a strip's camera target must be recomputed at render time,
    /// once the viewport width is known.
    camera_dirty: [bool; STRIP_COUNT],
    /// Newly created local editors must be fully visible on their first frame.
    camera_snap_pending: [bool; STRIP_COUNT],
    overview: bool,
    overview_progress: AnimatedValue,
    map_gesture: map_gesture::MapGesture,
    hints_overlay: bool,
    hints_progress: AnimatedValue,
    /// Presenter-friendly mode that briefly explains workspace actions on screen.
    showcase_mode: bool,
    showcase_cue: Option<ShowcaseCue>,
    showcase_task: Option<gpui::Task<()>>,
    /// Models which shortcuts the user knows, and teaches the ones they don't.
    coach: learning::Coach,
    learning_persistence: Option<learning::Persistence>,
    /// Retain the last hint through its exit animation.
    coach_display_hint: Option<learning::Hint>,
    /// Fade for the coach's tab chip.
    coach_progress: AnimatedValue,
    coach_expiry_task: Option<gpui::Task<()>>,
    /// Every non-archived session offered by the runtime, oldest to newest.
    sessions: Vec<jcode_sdk::SessionInfo>,
    /// What each swarm worker's own record says: model, effort, spend and what
    /// its newest message shows it doing. Keyed by session id; the catalog
    /// entry itself carries none of this.
    worker_meta: HashMap<String, jcode_sdk::WorkerMeta>,
    live_swarm_status: jcode_desktop_harness::swarm_status::LiveSwarmStatus,
    /// Past rows belong to this window only, never the durable session catalog.
    sidebar_closed_sessions: HashSet<String>,
    /// Sessions the shared daemon is running a turn for right now, from its
    /// streaming markers, with the sidebar mark shown while no panel is open.
    /// Marks persist across polls so the orb animates instead of restarting.
    daemon_running: HashMap<String, gpui::AnyView>,
    /// Never rerank an established shortcut, including after session refreshes.
    pinned_working_dir: Option<String>,
    /// Configured logins and API keys, refreshed in the background.
    accounts: Vec<accounts::Account>,
    /// Routing configuration with live usage, for the accounts sidebar.
    routing_view: Option<jcode_session_types::routing::RoutingView>,
    recent_accounts: Vec<String>,
    accounts_scroll: ScrollHandle,
    accounts_scroll_remainder: f32,
    accounts_layout_pending: bool,
    /// Review and redemption of one login's banked usage reset.
    status: String,
    connected: bool,
    focus_handle: FocusHandle,
    _navigation_fallback: gpui::Subscription,
    sidebar_scroll: ScrollHandle,
    sidebar_sessions_list: gpui::ListState,
    sidebar_session_layout: Vec<SidebarSessionLayout>,
    sidebar_projects: sidebar_projects::State,
    sidebar_selection: sidebar_selection::Selection,
    sidebar_gesture: Option<sidebar_gesture::Pending>,
    /// Session row under the pointer. Hovering swaps its spinner for actions.
    sidebar_hovered_session: Option<String>,
    /// Persisted todo progress (completed, total) per session, refreshed in
    /// the background with the session list. Open panels override it live.
    sidebar_todo_progress: HashMap<String, (usize, usize)>,
    catalog_tasks: crate::applet_view::CatalogTasks,
    unfinished_sessions: Vec<harness::UnfinishedSession>,
    /// Breathing titles of working sidebar sessions, each its own view.
    sidebar_title_pulses: crate::pulse_text::Tickers,
    /// Animated status glyphs of nested workers, one leaf view each.
    swarm_marks: crate::swarm_glyph::Marks,
    sidebar_navigation_scroll: ScrollHandle,
    live_tabs: live_tabs::TabMotion,
    sidebar_roller: sidebar_roller::Roller,
    /// Focus the active panel's input on the next render (set when panels
    /// appear from background updates, where no Window is available).
    focus_pending: bool,
    /// Active panel when highlight and keyboard focus were last reconciled.
    focus_synced_active: Option<gpui::EntityId>,
    /// When the last touchpad pan delta arrived. Drives the gesture reticle
    /// that marks where focus will land while a swipe is in flight.
    gesture_last: Option<Instant>,
    /// The sticky axis lock for the gesture currently on the strip.
    gesture: StripGesture,
    /// When the last precise delta of any axis arrived, so a stale gesture
    /// can be ended by silence when no Ended phase is delivered.
    gesture_seen: Option<Instant>,
    /// Settles horizontal strip pans onto the nearest panel edge.
    scroll_snap: scroll_snap::ScrollSnap,
    /// The folder picker inside the default directory panel. The picker that
    /// opens a folder lives in `modal_layer` instead.
    default_directory_picker: Option<Entity<FolderPicker>>,
    voice_settings: voice_settings::VoiceSettings,
    /// Rename, folder picker, usage reset and the beta notice live here, one
    /// at a time, each as its own view.
    modal_layer: Entity<ModalLayer>,
    focus_restore: FocusSnapshot,
    performance: Option<PerformanceProfile>,
    gpui_performance: GpuiPerformanceSnapshot,
    fps_counter: crate::fps_counter::FpsCounter,
    action_capture: Option<ActionCapture>,
    animation_tick_task: Option<gpui::Task<()>>,
    _bridge_task: gpui::Task<()>,
    _housekeeping_task: gpui::Task<()>,
    _stream_fixture: Option<gpui::Task<()>>,
    /// Offline screenshot fixture: keeps undelivered queued rows on screen.
    queued_fixture: Option<gpui::Task<()>>,
    _performance_task: gpui::Task<()>,
    _live_profile_task: gpui::Task<()>,
}

/// Key context of every workspace root. Chords only the workspace handles bind
/// here, so a terminal, composer or other child can claim a key before the
/// workspace does. See `bind_workspace_keys`.
pub(crate) const KEY_CONTEXT: &str = "Workspace";

impl Workspace {
    /// `Workspace`, plus `menu` while the rename dialog or the compact sidebar
    /// is open so `menu::Cancel` dismisses it wherever focus is.
    pub(super) fn key_context(&self) -> gpui::KeyContext {
        let mut context = gpui::KeyContext::default();
        context.add(KEY_CONTEXT);
        if self.compact_sidebar_open {
            context.add(crate::menu::CONTEXT);
        }
        context
    }

    fn new_modal_layer(cx: &mut Context<Self>) -> Entity<ModalLayer> {
        use gpui::AppContext as _;
        let layer = cx.new(|_| ModalLayer::default());
        // A modal restored after a reload has no earlier focus to return to.
        cx.subscribe(&layer, |this, _, _: &HiddenWithoutFocus, cx| {
            this.focus_pending = true;
            cx.notify();
        })
        .detach();
        layer
    }

    fn active_modal<V: ModalView>(&self, cx: &App) -> Option<Entity<V>> {
        self.modal_layer.read(cx).active::<V>()
    }

    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        host: HostHandle,
        snapshot: Option<WorkspaceSnapshot>,
        crash_launch: Option<jcode_desktop_api::WindowLaunch>,
    ) -> Self {
        crate::init_harness();
        // Launch work that must not wait for a pane: a packaged build whose
        // release state is still `Unknown` shows no update badge, so a
        // render-triggered release check would never start the automatic update
        // loop for a user who stays in the session view.
        updates::ensure_release_check();
        // Read the account file here so the first frame needs no disk access.
        sidebar_account::JcodeAccount::prime();
        let bridge = harness::spawn();
        let accounts_feed = accounts::spawn();
        // Offline fixtures arrive at once, so the first frame already lists
        // them instead of waiting for the housekeeping tick.
        let initial_accounts = if harness::screenshot_mode() {
            accounts_feed.latest().unwrap_or_default()
        } else {
            Vec::new()
        };
        let routing_feed = crate::routing::spawn();
        let recovering_crash = crash_launch.is_some();
        let launch = crash_launch
            .or_else(|| snapshot.as_ref()?.launch.clone().map(Into::into))
            .unwrap_or_else(|| {
                jcode_desktop_api::WindowLaunches::get(
                    cx,
                    window.window_handle().window_id().as_u64(),
                )
            });
        let performance_enabled = crate::performance::enabled(std::env::args_os());
        let resume_snapshot = snapshot.as_ref().map(|snapshot| snapshot.resume.clone());
        let single_panel = jcode_desktop_api::LaunchMode::from_args(&launch.args)
            == jcode_desktop_api::LaunchMode::SinglePanel;

        // Wake immediately when a bridge update arrives rather than polling an
        // empty channel at the display refresh rate.
        let update_bridge = bridge.clone();
        let bridge_task = cx.spawn(async move |this, cx| {
            while let Some(first) = update_bridge.recv().await {
                let updates = update_bridge.extend_batch(vec![first], 127);
                let alive = cx.update(|cx| {
                    let Some(workspace) = this.upgrade() else {
                        return false;
                    };
                    Self::dispatch_bridge_batch(&workspace, updates, cx);
                    true
                });
                if !alive {
                    break;
                }
                // recv() is immediately ready while a producer has a backlog.
                // Bound each batch and yield to input/layout between batches,
                // without reintroducing idle polling or dropping stream events.
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            }
        });

        let housekeeping_bridge = bridge.clone();
        let session_refresh_interval = crate::config::get().session_refresh_interval();
        let window_handle = window.window_handle();
        let (focus_wake, focus_changes) = async_channel::bounded(1);
        let initially_focused = window.is_window_active();
        if initially_focused {
            housekeeping_bridge.send(Command::SetCatalogForeground(true));
        }
        // Streaming markers are local files, so only poll them for a real
        // local daemon, never in tests or offline screenshot fixtures.
        let poll_daemon_running = !cfg!(test) && !harness::screenshot_mode();
        // Also applied by the host at startup. Repeating it here lets a
        // hot-reloaded UI fix an older, still-running host process.
        crate::memory::disable_transparent_huge_pages();
        let housekeeping_task = cx.spawn(async move |this, cx| {
            let mut catalog = housekeeping::CatalogRefresh::new(Instant::now(), session_refresh_interval);
            let mut focused = initially_focused;
            let mut last_update_state = updates::current();
            let mut last_release_state = updates::release_status();
            let mut last_check_visible = updates::user_check_visible();
            loop {
                let delay = if focused { housekeeping::FOREGROUND_TICK } else { housekeeping::BACKGROUND_TICK };
                let activated = futures_lite::future::race(
                    async { focus_changes.recv().await.is_ok() },
                    async { cx.background_executor().timer(delay).await; false },
                ).await;
                let Ok(is_active) = window_handle.update(cx, |_, window, _| window.is_window_active()) else {
                    break;
                };
                focused = is_active;
                if crate::memory::claim_trim_slot() {
                    cx.background_executor()
                        .spawn(async { crate::memory::trim() })
                        .detach();
                }
                let update_state = updates::current();
                let release_state = updates::release_status();
                let check_visible = updates::user_check_visible();
                if update_state != last_update_state
                    || release_state != last_release_state
                    || check_visible != last_check_visible
                {
                    last_update_state = update_state;
                    last_release_state = release_state;
                    last_check_visible = check_visible;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
                if focused && poll_daemon_running {
                    let running = cx
                        .background_executor()
                        .spawn(async { daemon_running_sessions() })
                        .await;
                    if this
                        .update(cx, |workspace: &mut Workspace, cx| {
                            workspace.set_daemon_running(running, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                if let Some(view) = routing_feed.latest() {
                    let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                        if workspace.routing_view.as_ref() != Some(&view) {
                            workspace.routing_view = Some(view);
                            workspace.accounts_layout_pending = true;
                            cx.notify();
                        }
                    });
                    if outcome.is_err() {
                        break;
                    }
                }
                let accounts = accounts_feed.latest();
                let picker_open = focused && this.read_with(cx, |w, _| w.resume.is_some()).unwrap_or(false);
                if catalog.due(Instant::now(), focused, activated, picker_open) {
                    housekeeping_bridge.send(Command::RefreshSessions);
                }
                let Some(accounts) = accounts else {
                    continue;
                };
                let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                    if workspace.accounts != accounts {
                        cx.set_global(crate::panel::usage::StatusAccounts(accounts.clone()));
                        for slot in &workspace.slots {
                            slot.panel.update_chat(cx, |_, cx| cx.notify());
                        }
                        workspace.accounts = accounts;
                        workspace.accounts_layout_pending = true;
                        cx.notify();
                    }
                });
                if outcome.is_err() {
                    break;
                }
            }
        });

        let performance_task = cx.spawn(async move |this, cx| {
            if !performance_enabled {
                return;
            }
            const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
            const DISPLAY_INTERVAL: Duration = Duration::from_millis(250);
            let mut last_wake = Instant::now();
            let mut last_display = Instant::now();
            loop {
                cx.background_executor().timer(SAMPLE_INTERVAL).await;
                let now = Instant::now();
                let wake_lag = now
                    .duration_since(last_wake)
                    .saturating_sub(SAMPLE_INTERVAL);
                last_wake = now;
                let refresh_display = last_display.elapsed() >= DISPLAY_INTERVAL;
                if refresh_display {
                    last_display = now;
                }
                let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                    if let Some(profile) = workspace.performance.as_mut() {
                        profile.observe_wake_lag(wake_lag);
                        if refresh_display {
                            cx.notify();
                        }
                    }
                });
                if outcome.is_err() {
                    break;
                }
            }
        });

        let _ = window;
        let mut workspace = Self {
            window_background: None,
            global_voice: Default::default(),
            voice_key: Default::default(),
            last_voice_chat: None,
            _voice_activation: Some(cx.observe_window_activation(window, move |this, window, cx| {
                let active = window.is_window_active();
                let _ = focus_wake.try_send(());
                crate::openrig_feed::window_activation_changed(cx);
                this.bridge.send(Command::SetCatalogForeground(active));
                this.global_voice.set_active(active);
                if !active {
                    if this.voice_key.is_down() {
                        this.end_voice_hold(&EndVoiceHold, window, cx);
                    }
                    this.voice_key = Default::default();
                }
            })),
            #[cfg(unix)]
            desktop_control: None,
            #[cfg(unix)]
            desktop_control_task: None,
            control_drafts: HashMap::new(),
            preview_control: None,
            preview_task: None,
            side_panel_snapshots: HashMap::new(),
            applet_toast: None,
            applet_consent_dismissed: HashSet::new(),
            applet_text_selection: None,
            sidebar_applets: None,
            sidebar_chrome: None,
            sidebar_panel_metadata: None,
            panel_metadata_observers: HashMap::new(),
            bridge,
            host,
            single_panel,
            launch: launch.clone(),
            resume: None,
            account_sign_in: account_sign_in::State::startup(),
            account_menu: account_menu::State::startup(),
            show_minimap: false,
            layout_mode: crate::config::get().appearance.layout_mode,
            folder_frame: Default::default(),
            sidebar_custom_width: SIDEBAR_WIDTH,
            sidebar_resizing: false,
            show_sidebar: sidebar_enabled(&launch.args, crate::config::get().workspace.sidebar),
            compact_sidebar_open: false,
            compact_sidebar_motion: responsive::VisibilityMotion::default(),
            last_canvas_width: None,
            // Offline captures can open on the accounts list directly.
            sidebar_view: if crate::harness::screenshot_mode()
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_SIDEBAR").as_deref() == Ok("accounts")
            {
                SidebarView::Accounts
            } else {
                SidebarView::Sessions
            },
            session_catalog: Default::default(),
            worktrees: sidebar_worktrees::State::default(),
            collections: Default::default(),
            pending_collection_drafts: Vec::new(),
            tutorial_page: 0,
            onboarding_launch: onboarding_simulator::LaunchState::default(),
            expanded_directories: HashSet::new(),
            file_tree: Default::default(),
            files_pane: Default::default(),
            expanded_lines: HashSet::new(),
            slots: Vec::new(),
            active: 0,
            active_row: 0,
            row_focus: [None; STRIP_COUNT],
            outgoing_row: None,
            row_origin: 0.0,
            row_progress: AnimatedValue::new(1.0, transition::policy(Transition::Row).duration),
            workspace_pull: workspace_pull::PullPreview::default(),
            previous: None,
            camera_x: [0.0; STRIP_COUNT],
            camera_target: [0.0; STRIP_COUNT],
            camera_from: [0.0; STRIP_COUNT],
            camera_started: [None; STRIP_COUNT],
            camera_touch_pan: [false; STRIP_COUNT],
            camera_dirty: [true; STRIP_COUNT],
            camera_snap_pending: [false; STRIP_COUNT],
            overview: false,
            map_gesture: Default::default(),
            overview_progress: AnimatedValue::new(
                0.0,
                transition::policy(Transition::Overview).duration,
            ),
            hints_overlay: false,
            hints_progress: AnimatedValue::new(0.0, transition::policy(Transition::Hints).duration),
            showcase_mode: crate::config::get().workspace.showcase_keys,
            showcase_cue: None,
            showcase_task: None,
            coach: learning::load(),
            learning_persistence: Some(learning::Persistence::spawn()),
            coach_progress: AnimatedValue::new(0.0, transition::policy(Transition::Coach).duration),
            coach_expiry_task: None,
            coach_display_hint: None,
            sessions: Vec::new(),
            worker_meta: HashMap::new(),
            live_swarm_status: Default::default(),
            sidebar_closed_sessions: HashSet::new(),
            daemon_running: HashMap::new(),
            pinned_working_dir: crate::config::get().workspace.pinned_working_dir.clone(),
            remotes: remotes::Machines::from_config(),
            accounts: initial_accounts,
            routing_view: None,
            recent_accounts: Vec::new(),
            accounts_scroll: ScrollHandle::new(),
            accounts_scroll_remainder: 0.0,
            accounts_layout_pending: true,
            status: "starting...".into(),
            connected: false,
            focus_handle: cx.focus_handle().tab_stop(true),
            _navigation_fallback: Self::install_navigation_fallback(cx),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_sessions_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(100.0)),
            sidebar_session_layout: Vec::new(),
            sidebar_projects: Default::default(),
            sidebar_selection: Default::default(),
            sidebar_gesture: None,
            sidebar_hovered_session: None,
            sidebar_todo_progress: Default::default(),
            catalog_tasks: Default::default(),
            unfinished_sessions: Vec::new(),
            sidebar_title_pulses: Default::default(),
            swarm_marks: Default::default(),
            sidebar_navigation_scroll: ScrollHandle::new(),
            live_tabs: live_tabs::TabMotion::default(),
            sidebar_roller: sidebar_roller::Roller::default(),
            focus_pending: false,
            focus_synced_active: None,
            gesture_last: None,
            gesture: StripGesture::default(),
            gesture_seen: None,
            scroll_snap: Default::default(),
            default_directory_picker: None,
            voice_settings: Default::default(),
            modal_layer: Self::new_modal_layer(cx),
            focus_restore: FocusSnapshot::Workspace,
            performance: performance_enabled.then(PerformanceProfile::default),
            gpui_performance: GpuiPerformanceSnapshot::default(),
            fps_counter: crate::fps_counter::FpsCounter::default(),
            action_capture: ActionCapture::from_env(),
            animation_tick_task: None,
            _bridge_task: bridge_task,
            _housekeeping_task: housekeeping_task,
            _stream_fixture: None,
            queued_fixture: None,
            _performance_task: performance_task,
            _live_profile_task: crate::live_profile::spawn(window, cx),
        };
        if snapshot.is_none()
            && !single_panel
            && !(harness::screenshot_mode()
                && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_HIDE_BETA").is_some())
        {
            workspace.show_beta_notice(window, cx);
        }
        if let Some(snapshot) = snapshot {
            workspace.apply_snapshot(snapshot, cx);
        } else if harness::screenshot_mode()
            && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_CLOUD_PICKER").is_some()
        {
            workspace.show_cloud_picker_fixture(cx);
        } else if harness::screenshot_mode()
            && let Some(variant) = std::env::var_os("JCODE_DESKTOP_SCREENSHOT_MACHINES")
        {
            workspace.show_machines_fixture(variant == "paired", cx);
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_PREVIEW_STATE").is_ok()
        {
            if let Ok(state) = std::env::var("JCODE_DESKTOP_SCREENSHOT_PREVIEW_STATE")
                .unwrap()
                .parse()
            {
                workspace.open_preview_unchecked(state, cx);
            }
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_CLOUD_STARTUP").is_ok()
        {
            workspace.remotes.default_host = Some(remotes::MANAGED_CLOUD_HOST.into());
            workspace.open_startup_draft(cx);
            workspace.start_default_startup(cx);
            // Offline fixture: show real managed-cloud phases, never contact the service.
            let phases: &[&str] = if std::env::var("JCODE_DESKTOP_SCREENSHOT_CLOUD_STARTUP")
                .as_deref()
                == Ok("failed")
            {
                &[
                    remotes::MANAGED_CLOUD_STARTING,
                    "Jcode Cloud requires an active paid Jcode subscription.",
                ]
            } else {
                &[
                    remotes::MANAGED_CLOUD_STARTING,
                    "Creating your Jcode Cloud machine…",
                    "Installing Jcode on your cloud machine…",
                ]
            };
            for (index, message) in phases.iter().enumerate() {
                let failed = index + 1 == phases.len() && phases.len() == 2;
                workspace.update_pending_remote_status(
                    Some(Panel::STARTUP_SESSION_ID),
                    message,
                    failed,
                    cx,
                );
            }
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_PENDING").as_deref() == Ok("1")
        {
            workspace.remotes.default_host = Some("offline-remote".into());
            workspace.open_startup_draft(cx);
            workspace.start_default_startup(cx);
            workspace.update_startup_status(
                "offline-remote: connection failed (offline fixture)",
                true,
                cx,
            );
        } else if harness::controlled_runtime_socket().is_some() {
            if let Ok(session_id) = std::env::var("JCODE_DESKTOP_CONTROL_RUNTIME_SESSION")
                && let Ok(session) =
                    serde_json::from_value::<jcode_sdk::SessionInfo>(serde_json::json!({
                        "session_id": session_id,
                        "status": "idle",
                    }))
            {
                workspace.sessions = vec![session.clone()];
                workspace.active = workspace.open_session(session, cx);
            }
        } else if crate::fixture_control_targets::draft_session() {
            workspace.connected = true;
            workspace.open_local_draft(workspace.default_working_dir(), cx);
        } else if harness::screenshot_mode() {
            let worktree_fixture = crate::fixture_control_targets::enabled()
                .then(|| std::env::var("JCODE_DESKTOP_CONTROL_SWARM_WORKTREE").ok())
                .flatten();
            let file_mentions = std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT")
                .as_deref()
                == Ok("file-mentions");
            let session = jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: "screenshot-fixture".into(),
                // Keep this longer than the sidebar to exercise single-line ellipsis.
                title: Some("Review markdown rendering with a very long session title".into()),
                working_dir: (worktree_fixture.is_some()
                    || crate::fixture_control_targets::files_panel()
                    || file_mentions)
                    .then(|| std::env::var("JCODE_DESKTOP_WORKING_DIR").ok())
                    .flatten()
                    .or_else(|| Some("/workspace/example".into())),
                status: "idle".into(),
                transcript_bytes: None,
                saved: false,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                save_label: None,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                context: None,
                edit_stats: Some(jcode_sdk::SessionEditStats {
                    added: 128,
                    removed: 37,
                    approximate: false,
                }),
            };
            workspace.connected = true;
            workspace.sessions = vec![session.clone()];
            workspace.active = workspace.open_session(session, cx);
            workspace.init_worktree_fixture(cx);
            workspace.init_collections_fixture();
            if crate::fixture_control_targets::enabled()
                && let Ok(session_id) = std::env::var("JCODE_DESKTOP_CONTROL_TODO_SESSION")
                && let Some(home) = harness::jcode_home()
            {
                let mut session = workspace.sessions[0].clone();
                session.session_id = session_id;
                session.title = Some("Verify a background project".into());
                session.working_dir = Some("/workspace/side-project".into());
                session.transcript_bytes = Some(65_600_000);
                session.user_connected = true;
                session.context = Some(jcode_sdk::api::SessionCatalogContext {
                    activity: Some(jcode_sdk::api::SessionCatalogActivity {
                        origin: jcode_sdk::api::SessionOrigin::Agent,
                        last_agent_activity_at_ms: Some(42),
                        last_message_at_ms: Some(Some(42)),
                    }),
                    ..Default::default()
                });
                workspace.sidebar_todo_progress =
                    harness::todo_progress_in(&home, std::slice::from_ref(&session));
                workspace.sessions.push(session);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_SWARM").as_deref() == Ok("real") {
                workspace.load_real_swarm_fixture(cx);
            } else if std::env::var_os("JCODE_DESKTOP_SCREENSHOT_SWARM").is_some() {
                for (index, (label, status, model, provider, effort)) in [
                    (
                        "API reviewer",
                        "working",
                        "claude-opus-5-5",
                        "claude-oauth",
                        "high",
                    ),
                    (
                        "Sidebar implementation",
                        "working",
                        "gpt-5.6",
                        "openai",
                        "low",
                    ),
                    (
                        "Test runner",
                        "completed",
                        "deepseek-v4.1-flash",
                        "opencode-go",
                        "max",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut child = workspace.sessions[0].clone();
                    child.session_id = format!("swarm-fixture-{index}");
                    child.title = None;
                    child.agent_label = Some(label.into());
                    if index == 0
                        && let Some(directory) = &worktree_fixture
                    {
                        child.working_dir = Some(directory.clone());
                        // Older daemons supplied the seeded todo title in the
                        // catalog but no live agent label for auto-lanes.
                        child.title = Some("btw-enter".into());
                        child.agent_label = None;
                    }
                    child.parent_session_id = Some("screenshot-fixture".into());
                    child.swarm_status = Some(status.into());
                    if index == 2 && worktree_fixture.is_some() {
                        child.title = Some("Retry after the previous worker failed".into());
                    }
                    child.edit_stats = Some(jcode_sdk::SessionEditStats {
                        added: 14 + index as u64 * 50,
                        removed: 2 + index as u64 * 8,
                        approximate: index == 1,
                    });
                    // The rows draw each worker's model and its reasoning rung,
                    // so the fixture must carry a record or the screenshot
                    // shows a bare name and proves nothing about the dots.
                    workspace.worker_meta.insert(
                        child.session_id.clone(),
                        jcode_sdk::WorkerMeta {
                            model: Some(model.into()),
                            provider: Some(provider.into()),
                            effort: Some(effort.into()),
                            input_tokens: Some(12_400 + index as u64 * 3_000),
                            output_tokens: Some(3_100 + index as u64 * 800),
                            ..jcode_sdk::WorkerMeta::default()
                        },
                    );
                    workspace.sessions.push(child);
                }
            }
            if worktree_fixture.is_some() {
                workspace.sync_swarm_strips(cx);
            }
            if let Ok(effort) = std::env::var("JCODE_DESKTOP_SCREENSHOT_BAR") {
                // Hand the swarm fixture's workers to the bar now; the strips
                // otherwise sync on the first session-list update.
                workspace.sync_swarm_strips(cx);
                if let Some(slot) = workspace.slots.first() {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.apply_bar_fixture(&effort, cx);
                        if crate::fixture_control_targets::enabled() {
                            match std::env::var("JCODE_DESKTOP_SCREENSHOT_CACHE_CARD").as_deref() {
                                Ok("1") => panel.apply_cache_card_fixture(false, cx),
                                Ok("warning") => panel.apply_cache_card_fixture(true, cx),
                                _ => {}
                            }
                            // Builds on the fixture above, so the cache card
                            // and the chip can be reviewed after a switch.
                            if let Ok(stage) = std::env::var("JCODE_DESKTOP_SCREENSHOT_MODEL_SWITCH") {
                                panel.apply_model_switch_fixture(&stage, cx);
                            }
                        }
                    });
                }
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("publish") {
                workspace.open_publish_fixture(cx);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("applet") {
                workspace.open_applet_fixture(cx);
            }
            let panel_count = std::env::var("JCODE_DESKTOP_SCREENSHOT_PANELS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 12);
            let panel_count = if single_panel { 1 } else { panel_count };
            let panel_titles = std::env::var("JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES").ok();
            let panel_title = |index: usize| {
                panel_titles
                    .as_deref()
                    .and_then(|titles| titles.split(',').nth(index))
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_owned)
            };
            if let Some(title) = panel_title(0) {
                workspace.sessions[0].title = Some(title.clone());
                if let Some(slot) = workspace.slots.first() {
                    slot.panel
                        .update_chat(cx, |panel, _| panel.title = title.into());
                }
            }
            for index in 1..panel_count {
                let mut session = workspace.sessions[0].clone();
                session.session_id = format!("screenshot-fixture-{index}");
                session.title = Some(
                    panel_title(index).unwrap_or_else(|| format!("Review folder {}", index + 1)),
                );
                workspace.sessions.push(session.clone());
                workspace.active = workspace.open_session(session, cx);
            }
            if panel_count > 1
                || std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("openrig")
            {
                for slot in &mut workspace.slots {
                    slot.width_fraction = 1.0 / panel_count as f32;
                    slot.animated_width = AnimatedValue::new(
                        slot.width_fraction,
                        transition::policy(Transition::PanelWidth).duration,
                    );
                }
                workspace.active = panel_count / 2;
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TAB_STATUSES").as_deref() == Ok("1") {
                workspace.seed_tab_status_fixture(cx);
            }
            if let Some(path) = std::env::var_os("JCODE_DESKTOP_SCREENSHOT_MARKDOWN_FILE") {
                let file = cx.new(|cx| FilePanel::new(PathBuf::from(path), cx));
                workspace.slots[workspace.active].panel = PanelItem::File(file);
            }
            workspace.focus_pending = true;
            if workspace_stream_fixture::enabled() {
                workspace._stream_fixture = Some(workspace_stream_fixture::spawn(
                    "screenshot-fixture".into(),
                    cx,
                ));
            }
            if let Some(count) = std::env::var("JCODE_DESKTOP_SCREENSHOT_QUEUED")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|count| *count > 0)
            {
                // Re-seed on a timer: the offline demo turn ends, and its
                // turn_done drains the queue, so a one-shot seed would show the
                // rows for a frame and then hand them to the transcript.
                // `JCODE_DESKTOP_SCREENSHOT_QUEUE_EDIT=1` also opens the first
                // row in its inline editor, with a screenshot and files.
                let queue_edit =
                    std::env::var("JCODE_DESKTOP_SCREENSHOT_QUEUE_EDIT").as_deref() == Ok("1");
                workspace.queued_fixture = Some(cx.spawn(async move |this, cx| {
                    let executor = cx.background_executor().clone();
                    let mut round = 0usize;
                    loop {
                        let mut seeded = false;
                        this.update(cx, |workspace, cx| {
                            if let Some(slot) = workspace.slots.first() {
                                slot.panel.update_chat(cx, |panel, cx| {
                                    if !panel.has_visible_queued_prompts() {
                                        panel.seed_screenshot_queued(count, round);
                                        if queue_edit {
                                            panel.seed_screenshot_queue_edit(cx);
                                        }
                                        seeded = true;
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .ok();
                        if seeded {
                            round += 1;
                        }
                        executor.timer(Duration::from_millis(1500)).await;
                    }
                }));
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_SIDEBAR").as_deref() == Ok("accounts") {
                workspace.sidebar_view = SidebarView::Accounts;
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_ACCOUNTS").as_deref() == Ok("1")
                && let Some(slot) = workspace.slots.first()
            {
                let session = slot.panel.session_id(cx).to_owned();
                let bridge = workspace.bridge.clone();
                let panel = cx.new(|cx| Panel::new_accounts(&session, None, bridge, cx));
                workspace.slots[0].panel = PanelItem::Chat(panel);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_HISTORY").as_deref() == Ok("1") {
                for index in 0..80 {
                    let mut session = workspace.sessions[0].clone();
                    session.session_id = format!("screenshot-history-{index:02}");
                    session.title = Some(format!("History session {index:02}"));
                    workspace.sessions.push(session);
                }
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_NOTIFICATION").as_deref() == Ok("1") {
                workspace.showcase_cue = Some(ShowcaseCue {
                    shortcut: "Super + Shift + S".into(),
                    action: "Showcase mode on",
                    tutorial_group: "",
                    started_at: None,
                });
            }
            if let Some(stage) = std::env::var("JCODE_DESKTOP_SCREENSHOT_LEARN_STAGE")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|stage| (1..=3).contains(stage))
            {
                workspace.sidebar_view = SidebarView::Learn;
                workspace.tutorial_page = stage - 1;
            }
        } else {
            workspace.open_startup_draft(cx);
        }
        workspace.finish_launch(resume_snapshot, recovering_crash, window, cx);
        if workspace.resume.is_none() && workspace.remotes.default_host.is_some() {
            workspace.start_default_startup(cx);
        }
        workspace.start_preview_control(cx);
        #[cfg(unix)]
        workspace.start_desktop_control(window, cx);
        workspace.start_global_voice(window, cx);
        workspace.detect_account_imports(cx);
        workspace.load_collections(cx);
        workspace.open_collections_fixture_form(window, cx);
        workspace
    }

    fn finish_launch(
        &mut self,
        resume_snapshot: Option<resume::Snapshot>,
        recovering_crash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use jcode_desktop_api::LaunchMode;
        let apply_launch = resume_snapshot.is_none() || recovering_crash;
        let resume_requested = LaunchMode::resume_requested(&self.launch.args);
        // A crash restores tabs, then applies this launch's arguments. Clean
        // quit, restart manifests and in-process reloads keep their saved UI.
        if recovering_crash && resume_requested {
            self.open_resume(&OpenResume, window, cx);
        } else {
            self.restore_resume(resume_snapshot, resume_requested, window, cx);
        }
        if apply_launch && let Some(session_id) = LaunchMode::requested_session(&self.launch.args) {
            self.resume = None;
            self.open_requested_session(session_id, window, cx);
            self.focus_restore = FocusSnapshot::Workspace;
        }
    }

    fn open_startup_draft(&mut self, cx: &mut Context<Self>) {
        self.active = self.open_session(
            jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: Panel::STARTUP_SESSION_ID.into(),
                title: Some(self.remotes.default_host.as_ref().map_or_else(
                    || "New session".into(),
                    |host| format!("Connecting to {host}"),
                )),
                working_dir: self
                    .remotes
                    .default_host
                    .is_none()
                    .then(|| {
                        launch_directory_override(&self.launch)
                            .or_else(|| self.new_session_directory())
                    })
                    .flatten(),
                status: "starting".into(),
                transcript_bytes: None,
                saved: false,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                save_label: None,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                edit_stats: None,
                context: None,
            },
            cx,
        );
        // The first frame is a full-sized editor, not a zero-width entrance
        // animation that makes typing appear to go nowhere.
        let slot = &mut self.slots[self.active];
        slot.animated_width = AnimatedValue::new(
            slot.width_fraction,
            transition::policy(Transition::PanelOpen).duration,
        );
        self.focus_pending = true;
    }

    #[cfg(test)]
    pub(crate) fn enable_test_minimap(&mut self) {
        self.show_minimap = true;
    }

    /// A workspace with no runtime and a caller-supplied coach, for tests that
    /// drive real keystrokes through the real keymap.
    #[cfg(test)]
    pub fn for_test(coach: learning::Coach, cx: &mut Context<Self>) -> Self {
        Self {
            window_background: None,
            global_voice: Default::default(),
            voice_key: Default::default(),
            last_voice_chat: None,
            _voice_activation: None,
            #[cfg(unix)]
            desktop_control: None,
            #[cfg(unix)]
            desktop_control_task: None,
            control_drafts: HashMap::new(),
            preview_control: None,
            preview_task: None,
            side_panel_snapshots: HashMap::new(),
            applet_toast: None,
            applet_consent_dismissed: HashSet::new(),
            applet_text_selection: None,
            sidebar_applets: None,
            sidebar_chrome: None,
            sidebar_panel_metadata: None,
            panel_metadata_observers: HashMap::new(),
            bridge: harness::spawn_inert(),
            host: HostHandle::inert(),
            single_panel: false,
            launch: Default::default(),
            resume: None,
            account_sign_in: account_sign_in::State::default(),
            account_menu: account_menu::State::default(),
            show_sidebar: true,
            sidebar_custom_width: SIDEBAR_WIDTH,
            sidebar_resizing: false,
            show_minimap: false,
            layout_mode: crate::config::LayoutMode::FolderTabs,
            folder_frame: Default::default(),
            compact_sidebar_open: false,
            compact_sidebar_motion: responsive::VisibilityMotion::default(),
            last_canvas_width: None,
            sidebar_view: SidebarView::Sessions,
            session_catalog: Default::default(),
            worktrees: sidebar_worktrees::State::default(),
            collections: Default::default(),
            pending_collection_drafts: Vec::new(),
            tutorial_page: 0,
            onboarding_launch: onboarding_simulator::LaunchState::default(),
            expanded_directories: HashSet::new(),
            file_tree: Default::default(),
            files_pane: Default::default(),
            expanded_lines: HashSet::new(),
            slots: Vec::new(),
            active: 0,
            active_row: 0,
            row_focus: [None; STRIP_COUNT],
            outgoing_row: None,
            row_origin: 0.0,
            row_progress: AnimatedValue::new(1.0, transition::policy(Transition::Row).duration),
            workspace_pull: workspace_pull::PullPreview::default(),
            previous: None,
            camera_x: [0.0; STRIP_COUNT],
            camera_target: [0.0; STRIP_COUNT],
            camera_from: [0.0; STRIP_COUNT],
            camera_started: [None; STRIP_COUNT],
            camera_touch_pan: [false; STRIP_COUNT],
            camera_dirty: [true; STRIP_COUNT],
            camera_snap_pending: [false; STRIP_COUNT],
            overview: false,
            map_gesture: Default::default(),
            overview_progress: AnimatedValue::new(
                0.0,
                transition::policy(Transition::Overview).duration,
            ),
            hints_overlay: false,
            hints_progress: AnimatedValue::new(0.0, transition::policy(Transition::Hints).duration),
            showcase_mode: true,
            showcase_cue: None,
            showcase_task: None,
            coach,
            learning_persistence: None,
            coach_progress: AnimatedValue::new(0.0, transition::policy(Transition::Coach).duration),
            coach_expiry_task: None,
            coach_display_hint: None,
            sessions: Vec::new(),
            worker_meta: HashMap::new(),
            live_swarm_status: Default::default(),
            sidebar_closed_sessions: HashSet::new(),
            daemon_running: HashMap::new(),
            pinned_working_dir: None,
            remotes: remotes::Machines::default(),
            accounts: Vec::new(),
            routing_view: None,
            recent_accounts: Vec::new(),
            accounts_scroll: ScrollHandle::new(),
            accounts_scroll_remainder: 0.0,
            accounts_layout_pending: true,
            status: "test".into(),
            connected: true,
            focus_handle: cx.focus_handle().tab_stop(true),
            _navigation_fallback: Self::install_navigation_fallback(cx),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_sessions_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(100.0)),
            sidebar_session_layout: Vec::new(),
            sidebar_projects: Default::default(),
            sidebar_selection: Default::default(),
            sidebar_gesture: None,
            sidebar_hovered_session: None,
            sidebar_todo_progress: Default::default(),
            catalog_tasks: Default::default(),
            unfinished_sessions: Vec::new(),
            sidebar_title_pulses: Default::default(),
            swarm_marks: Default::default(),
            sidebar_navigation_scroll: ScrollHandle::new(),
            live_tabs: live_tabs::TabMotion::default(),
            sidebar_roller: sidebar_roller::Roller::default(),
            focus_pending: false,
            focus_synced_active: None,
            gesture_last: None,
            gesture: StripGesture::default(),
            gesture_seen: None,
            scroll_snap: Default::default(),
            default_directory_picker: None,
            voice_settings: Default::default(),
            modal_layer: Self::new_modal_layer(cx),
            focus_restore: FocusSnapshot::Workspace,
            performance: None,
            gpui_performance: GpuiPerformanceSnapshot::default(),
            fps_counter: crate::fps_counter::FpsCounter::default(),
            action_capture: None,
            animation_tick_task: None,
            _bridge_task: cx.spawn(async move |_, _| {}),
            _housekeeping_task: cx.spawn(async move |_, _| {}),
            _stream_fixture: None,
            queued_fixture: None,
            _performance_task: cx.spawn(async move |_, _| {}),
            _live_profile_task: cx.spawn(async move |_, _| {}),
        }
    }

    pub fn snapshot(&self, window: &Window, cx: &App) -> anyhow::Result<WorkspaceSnapshot> {
        self.snapshot_inner(window, cx, false)
    }

    pub fn snapshot_for_reload(
        &self,
        window: &Window,
        cx: &App,
    ) -> anyhow::Result<WorkspaceSnapshot> {
        // The old generation's panels are dropped next, ending any recording.
        if self
            .slots
            .iter()
            .any(|slot| slot.panel.read_chat(cx).is_some_and(Panel::voice_active))
        {
            eprintln!("voice: hot reload is replacing the UI while a recording is active");
        }
        let snapshot = self.snapshot_inner(window, cx, true)?;
        recovery::remember_reload_snapshot(&snapshot, window, cx);
        Ok(snapshot)
    }

    fn snapshot_inner(
        &self,
        window: &Window,
        cx: &App,
        for_reload: bool,
    ) -> anyhow::Result<WorkspaceSnapshot> {
        // Closing slots and transient tool snapshots must not be restored as
        // server sessions. All saved indices refer to persisted slots only.
        let slots: Vec<_> = self
            .slots
            .iter()
            .filter(|slot| {
                !slot.closing
                    && !slot.panel.is_changelog()
                    && slot.panel.read_chat(cx).is_none_or(|panel| {
                        !panel.is_change_review()
                            && !panel.is_accounts_panel()
                            && panel.preview_state.is_none()
                    })
            })
            .collect();
        let index_for_id =
            |id: gpui::EntityId| slots.iter().position(|slot| slot.panel.entity_id() == id);
        let review_source = self.slots.get(self.active).and_then(|slot| {
            if slot.panel.is_changelog() {
                return self.previous.and_then(index_for_id);
            }
            let panel = slot.panel.read_chat(cx)?;
            let source_id = panel
                .session_id
                .strip_prefix("review://")
                .or_else(|| panel.session_id.strip_prefix("accounts://"))?;
            slots
                .iter()
                .position(|slot| slot.panel.session_id(cx) == source_id)
        });
        let active = self
            .slots
            .get(self.active)
            .and_then(|slot| index_for_id(slot.panel.entity_id()))
            .or(review_source)
            .unwrap_or(0);
        let picker = self.folder_picker(cx);
        let focus = if picker
            .as_ref()
            .is_some_and(|picker| picker.focus_handle(cx).is_focused(window))
        {
            FocusSnapshot::FolderSearch
        } else if let Some(index) = slots
            .iter()
            .position(|slot| slot.panel.input_focus_handle(cx).is_focused(window))
        {
            FocusSnapshot::Panel(index)
        } else if let Some(index) = review_source {
            FocusSnapshot::Panel(index)
        } else {
            FocusSnapshot::Workspace
        };
        Ok(WorkspaceSnapshot {
            launch: Some((&self.launch).into()),
            format_version: SNAPSHOT_FORMAT_VERSION,
            resume: self.resume_snapshot(cx),
            side_panel_snapshots: self.side_panel_snapshots.clone(),
            layout_mode: self.layout_mode,
            recent_accounts: self.recent_accounts.clone(),
            sidebar_closed_sessions: if for_reload {
                self.sidebar_closed_sessions.clone()
            } else {
                HashSet::new()
            },
            sidebar_view: self.sidebar_view,
            session_catalog: self.session_catalog,
            tutorial_page: self.tutorial_page,
            slots: slots
                .iter()
                .map(|slot| SlotSnapshot {
                    panel: slot.panel.snapshot(for_reload, cx),
                    row: slot.row,
                    width_fraction: slot.width_fraction,
                    restore_fraction: slot.restore_fraction,
                })
                .collect(),
            active,
            active_row: if review_source.is_some() {
                slots[active].row
            } else {
                self.active_row
            },
            row_focus: self.row_focus.map(|id| id.and_then(index_for_id)),
            previous: self.previous.and_then(index_for_id),
            last_voice_chat: self.last_voice_chat.and_then(index_for_id),
            camera_x: self.camera_x,
            camera_target: self.camera_target,
            overview: self.overview,
            hints_overlay: self.hints_overlay,
            folder_picker_sets_default: picker.as_ref().is_some_and(|p| p.read(cx).sets_default),
            folder_picker_dir: picker.as_ref().map(|p| p.read(cx).dir.clone()),
            folder_picker_error: picker.as_ref().and_then(|p| p.read(cx).error.clone()),
            folder_search: picker
                .as_ref()
                .map(|p| p.read(cx).search.read(cx).snapshot()),
            focus,
        })
    }

    fn apply_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        self.side_panel_snapshots = snapshot.side_panel_snapshots;
        self.layout_mode = snapshot.layout_mode;
        self.recent_accounts = snapshot.recent_accounts;
        self.sidebar_closed_sessions = snapshot.sidebar_closed_sessions;
        self.sidebar_view = snapshot.sidebar_view;
        self.session_catalog = snapshot.session_catalog;
        self.tutorial_page = snapshot.tutorial_page.min(2);
        self.slots.clear();
        // Saved indices refer to the saved slot list. Some saved slots are
        // dropped below, so translate every index to the restored slot that
        // holds the same panel rather than whatever now sits at that index.
        let mut restored_index: Vec<Option<usize>> = Vec::with_capacity(snapshot.slots.len());
        for saved in snapshot.slots {
            restored_index.push(None);
            let mut panel_state = saved.panel;
            // Some(needs a session now) for a local draft from the last run.
            let mut restored_local_draft = None;
            // Preview fixture IDs are never runtime sessions, including old snapshots.
            if panel_state.session_id.starts_with("preview://")
                || panel_state.session_id.starts_with("accounts://")
                || panel_state.session_id == ChangelogPanel::SESSION_ID
                // The retired native inbox. Gmail is now an applet panel.
                || panel_state.session_id == "gmail://inbox"
            {
                continue;
            }
            if panel_state.session_id.starts_with("startup://draft/") {
                // The previous bridge generation cannot deliver its in-flight
                // result to this workspace. Restart with a fresh correlation ID
                // and original destination, not today's machine default.
                let help = panel_state.session_id.starts_with("startup://draft/help/");
                let publish = crate::publish::is_publish_draft(&panel_state.session_id);
                let remote_host =
                    pending::remote_draft_host(&panel_state.session_id).map(str::to_owned);
                if let Some(host) = remote_host {
                    let host = remotes::new_session_host(&host).to_owned();
                    panel_state.session_id = pending::next_remote_draft_id(&host);
                    self.create_remote_draft_session(host, panel_state.session_id.clone(), cx);
                } else if !panel_state
                    .session_id
                    .starts_with("startup://draft/remote/")
                {
                    panel_state.session_id = pending::next_draft_id();
                    if help {
                        panel_state.session_id = panel_state.session_id.replacen(
                            "startup://draft/",
                            "startup://draft/help/",
                            1,
                        );
                    } else if publish {
                        panel_state.session_id = panel_state.session_id.replacen(
                            "startup://draft/",
                            crate::publish::DRAFT_PREFIX,
                            1,
                        );
                    }
                    restored_local_draft = Some(help || publish);
                }
            }
            if panel_state.terminal_resource_id.is_some()
                || panel_state.session_id == PanelItem::TERMINAL_SESSION_ID
            {
                let host = self.host;
                let terminal = cx.new(|cx| {
                    TerminalPanel::new(
                        panel_state.working_dir,
                        panel_state.terminal_resource_id,
                        panel_state.terminal_output_cursor,
                        host,
                        cx,
                    )
                });
                *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
                let terminal = PanelItem::Terminal(terminal);
                self.slots.push(restored_slot(
                    terminal,
                    saved.row,
                    saved.width_fraction,
                    saved.restore_fraction,
                ));
                continue;
            }
            if let Some(path) = panel_state.session_id.strip_prefix("file://") {
                let file = cx.new(|cx| FilePanel::new(PathBuf::from(path), cx));
                *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
                self.slots.push(restored_slot(
                    PanelItem::File(file),
                    saved.row,
                    saved.width_fraction,
                    saved.restore_fraction,
                ));
                continue;
            }
            let panel = if panel_state.session_id == "unfinished-work" {
                let workspace = cx.weak_entity();
                cx.new(|cx| {
                    Panel::new_unfinished_work(
                        self.unfinished_sessions.clone(),
                        std::sync::Arc::new(move |session, window, cx| {
                            workspace
                                .update(cx, |workspace, cx| {
                                    workspace.activate_unfinished_session(session, window, cx);
                                })
                                .expect("unfinished-work workspace is still available");
                        }),
                        self.bridge.clone(),
                        cx,
                    )
                })
            } else if panel_state.side_document.is_some() {
                cx.new(|cx| {
                    Panel::side_document_from_snapshot(&panel_state, self.bridge.clone(), cx)
                        .expect("checked side document snapshot")
                })
            } else if let Some(instance) = panel_state
                .session_id
                .strip_prefix(crate::panel::applet_panel::APPLET_PREFIX)
            {
                cx.new(|cx| Panel::new_applet(instance.to_owned(), self.bridge.clone(), cx))
            } else if panel_state.session_id == "todoist://tasks" {
                cx.new(|cx| Panel::new_todoist(self.bridge.clone(), cx))
            } else if panel_state.session_id == crate::panel::openrig::SESSION_ID {
                cx.new(|cx| Panel::new_openrig(self.bridge.clone(), cx))
            } else if panel_state.session_id == crate::panel::orchestration::SESSION_ID {
                let opener = self.orchestration_opener(cx);
                cx.new(|cx| Panel::new_orchestration(opener, self.bridge.clone(), cx))
            } else {
                let session_id = panel_state.session_id.clone();
                let title = Some(panel_state.title.clone());
                let working_dir = panel_state.working_dir.clone();
                let panel = cx.new(|cx| {
                    Panel::new(
                        session_id.clone(),
                        title,
                        working_dir,
                        self.bridge.clone(),
                        cx,
                    )
                });
                if !Panel::is_pending_session_id(&session_id)
                    && session_id != Panel::DEFAULT_DIRECTORY_SESSION_ID
                    && session_id != Panel::MACHINES_SESSION_ID
                {
                    self.bridge.send(Command::Watch { session_id });
                }
                panel
            };
            if panel.read(cx).is_machines() {
                let input = self.create_machine_input(cx);
                panel.update(cx, |panel, _| panel.input = input.clone());
                self.remotes.input = Some(input);
            } else if !panel.read(cx).is_default_directory() {
                Self::wire_session_panel(&panel, cx);
            }
            panel.update(cx, |panel, cx| {
                panel.restore_snapshot(panel_state, cx);
                panel.prefetch_persisted_history(cx);
                if let Some(needs_session) = restored_local_draft {
                    panel.resume_draft_session(needs_session, cx);
                }
            });
            if panel
                .read(cx)
                .session_id
                .starts_with("startup://draft/remote/")
            {
                panel.update(cx, |panel, cx| {
                    panel.status = match pending::remote_draft_host(&panel.session_id) {
                        Some(host) if remotes::managed_destination(host) => {
                            panel.title = remotes::machine_label(host).to_owned().into();
                            format!("{} Your queued prompts are preserved.", remotes::MANAGED_CLOUD_STARTING)
                        }
                        Some(host) => format!("Reconnecting to {host}… Your queued prompts are preserved."),
                        None => "Session creation failed: invalid saved remote destination. No local session was opened.".into(),
                    };
                    cx.notify();
                });
            }
            *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
            self.slots.push(restored_slot(
                PanelItem::Chat(panel),
                saved.row,
                saved.width_fraction,
                saved.restore_fraction,
            ));
        }
        let restored = |index: usize| restored_index.get(index).copied().flatten();
        self.active_row = snapshot.active_row;
        // Stay on the saved panel. If it was not restorable, stay on the same
        // row rather than jumping to the first panel of the workspace.
        self.active = restored(snapshot.active)
            .or_else(|| snapshot.row_focus[self.active_row].and_then(restored))
            .or_else(|| {
                self.slots
                    .iter()
                    .position(|slot| slot.row == self.active_row)
            })
            .unwrap_or(0);
        self.row_focus = snapshot.row_focus.map(|index| {
            index
                .and_then(restored)
                .map(|index| self.slots[index].panel.entity_id())
        });
        self.previous = snapshot
            .previous
            .and_then(restored)
            .map(|index| self.slots[index].panel.entity_id());
        self.last_voice_chat = snapshot
            .last_voice_chat
            .and_then(restored)
            .map(|index| self.slots[index].panel.entity_id());
        self.camera_x = snapshot.camera_x;
        // Resume from what was actually visible. With no restored animation
        // clock, retaining the old destination would snap there on first paint.
        // The dirty camera resolves a fresh smooth transition if focus needs it.
        self.camera_target = snapshot.camera_x;
        self.camera_from = snapshot.camera_x;
        self.camera_started = [None; STRIP_COUNT];
        self.camera_touch_pan = [false; STRIP_COUNT];
        self.camera_dirty = [true; STRIP_COUNT];
        self.camera_snap_pending = [false; STRIP_COUNT];
        self.overview = snapshot.overview;
        self.overview_progress = AnimatedValue::new(
            if snapshot.overview { 1.0 } else { 0.0 },
            transition::policy(Transition::Overview).duration,
        );
        self.hints_overlay = snapshot.hints_overlay;
        self.hints_progress = AnimatedValue::new(
            if snapshot.hints_overlay { 1.0 } else { 0.0 },
            transition::policy(Transition::Hints).duration,
        );
        self.focus_restore = match snapshot.focus {
            FocusSnapshot::Panel(index) => restored(index)
                .map(FocusSnapshot::Panel)
                .unwrap_or(FocusSnapshot::Workspace),
            focus => focus,
        };
        if let Some(dir) = snapshot.folder_picker_dir {
            let sets_default = snapshot.folder_picker_sets_default;
            let workspace = cx.weak_entity();
            let picker = cx.new(|cx| {
                let mut picker = FolderPicker::new(workspace, sets_default, dir, None, cx);
                picker.error = snapshot.folder_picker_error;
                picker
            });
            if let Some(search_state) = snapshot.folder_search {
                let search = picker.read(cx).search.clone();
                search.update(cx, |search, cx| search.restore(search_state, cx));
            }
            if sets_default {
                if let Some(index) = self.default_directory_panel_index(cx) {
                    let search = picker.read(cx).search.clone();
                    self.slots[index]
                        .panel
                        .update_chat(cx, |panel, _| panel.input = search);
                }
                self.adopt_default_directory_picker(picker, cx);
            } else {
                self.modal_layer
                    .update(cx, |layer, cx| layer.restore(picker, cx));
            }
        }
        self.restore_workspace_live_views(cx);
        if self.machines_panel_index(cx).is_some() {
            self.refresh_cloud_registry(cx);
        }
    }

    pub fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &self.resume {
            window.focus(&state.search.focus_handle(cx), cx);
            return;
        }
        if self.account_sign_in.visible || self.onboarding_launch.error.is_some() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        match self.focus_restore.clone() {
            FocusSnapshot::Panel(_) if self.overview || self.overview_progress.is_animating() => {
                self.focus_active(window, cx);
            }
            FocusSnapshot::Panel(index) => {
                if let Some(slot) = self.slots.get(index) {
                    let handle = slot.panel.input_focus_handle(cx);
                    window.focus(&handle, cx);
                } else {
                    window.focus(&self.focus_handle, cx);
                }
            }
            FocusSnapshot::FolderSearch => {
                if let Some(picker) = self.folder_picker(cx) {
                    let handle = picker.focus_handle(cx);
                    window.focus(&handle, cx);
                } else {
                    self.focus_active(window, cx);
                }
            }
            FocusSnapshot::Workspace => self.focus_active(window, cx),
        }
    }

    /// Add a bare panel for tests, bypassing the runtime.
    #[cfg(test)]
    pub fn push_test_panel(&mut self, name: &str, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let panel =
            cx.new(|cx| Panel::new(name.to_string(), Some(name.to_string()), None, bridge, cx));
        Panel::connect_input(&panel, cx);
        self.slots.push(Slot {
            panel: PanelItem::Chat(panel),
            row: self.active_row,
            width_fraction: DEFAULT_WIDTH,
            animated_width: AnimatedValue::new(
                DEFAULT_WIDTH,
                transition::policy(Transition::PanelWidth).duration,
            ),
            order_offset: AnimatedValue::new(
                0.0,
                transition::policy(Transition::PanelOrder).duration,
            ),
            order_distance_fraction: DEFAULT_WIDTH,
            close_progress: AnimatedValue::new(
                1.0,
                transition::policy(Transition::PanelClose).duration,
            ),
            closing: false,
            restore_fraction: None,
        });
    }

    #[cfg(test)]
    pub fn set_test_bridge(&mut self, bridge: Bridge) {
        self.bridge = bridge;
    }

    /// Seed the routing view for tests, bypassing the runtime.
    #[cfg(test)]
    pub fn set_test_routing_view(&mut self, view: jcode_session_types::routing::RoutingView) {
        self.routing_view = Some(view);
    }

    /// Seed accounts for tests, bypassing the CLI.
    #[cfg(test)]
    pub fn set_test_accounts(&mut self, accounts: Vec<accounts::Account>) {
        self.accounts = accounts;
    }

    #[cfg(test)]
    pub fn test_coach(&self) -> &learning::Coach {
        &self.coach
    }

    #[cfg(test)]
    pub fn test_coach_mut(&mut self) -> &mut learning::Coach {
        &mut self.coach
    }

    /// The panel entity at `index`, so tests can drive real events through it.
    #[cfg(test)]
    pub fn test_panel(&self, index: usize) -> Option<Entity<Panel>> {
        self.slots
            .get(index)
            .and_then(|slot| slot.panel.chat().cloned())
    }

    #[cfg(test)]
    pub fn test_focus_position(&self) -> Option<usize> {
        self.row_indices(self.active_row)
            .position(|index| index == self.active)
    }

    fn dispatch_bridge_batch(workspace: &Entity<Self>, updates: Vec<Update>, cx: &mut App) {
        let mut changed = false;
        for update in updates {
            match &update {
                Update::Event { session_id, event } => {
                    crate::render_stats::note_variant("event:", event);
                    if matches!(event, jcode_sdk::ApiEvent::TextDelta { .. }
                        | jcode_sdk::ApiEvent::ReasoningDelta { .. }
                        | jcode_sdk::ApiEvent::ToolInputDelta { .. })
                    {
                        let workspace = workspace.read(cx);
                        let owner = workspace.side_question_owner(session_id, cx);
                        let panel = workspace.slots.iter().find(|slot| {
                            (owner.is_none() || !slot.closing)
                                && slot.panel.session_id(cx) == *session_id
                        }).and_then(|slot| slot.panel.chat().cloned());
                        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
                        if let Some(owner) = owner {
                            owner.update(cx, |panel, cx| panel.apply_side_question_event(event, cx));
                        }
                        if let Some(panel) = panel {
                            panel.update(cx, |panel, cx| panel.apply(event, cx));
                        }
                        continue;
                    }
                }
                other => crate::render_stats::note_variant("update:", other),
            }
            changed |= workspace.update(cx, |workspace, cx| workspace.apply(update, cx));
        }
        if changed {
            crate::render_stats::note("workspace-notify:bridge");
            cx.notify(workspace.entity_id());
        }
    }

    /// Apply a bridge update and report whether it changed visible workspace state.
    ///
    /// Session refreshes arrive every two seconds. Most contain the exact same
    /// SDK snapshot, so treating them as changes would rebuild the complete
    /// sidebar and every open panel for no user-visible result.
    pub(crate) fn apply(&mut self, mut update: Update, cx: &mut Context<Self>) -> bool {
        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
        if let Update::Sessions { sessions } = &mut update {
            // A desktop window can disappear while its daemon-owned sessions keep
            // running. Those panels are restored from the workspace snapshot before
            // the asynchronous session catalog arrives. Older runtimes and bounded
            // catalogs can omit them, so never let a refresh erase an open session
            // from the sidebar.
            for slot in &self.slots {
                let Some(panel) = slot.panel.read_chat(cx) else {
                    continue;
                };
                if !panel.session_id.starts_with("session_")
                    || sessions
                        .iter()
                        .any(|session| session.session_id == panel.session_id)
                {
                    continue;
                }
                if let Some(known) = self
                    .sessions
                    .iter()
                    .find(|s| s.session_id == panel.session_id)
                {
                    sessions.push(known.clone());
                    continue;
                }
                sessions.push(jcode_sdk::SessionInfo {
                    user_connected: false,
                    session_id: panel.session_id.clone(),
                    working_dir: panel.working_dir.clone(),
                    title: Some(panel.title.to_string()),
                    status: "active".into(),
                    transcript_bytes: None,
                    saved: false,
                    updated_at_ms: None,
                    last_active_at_ms: None,
                    archived: false,
                    archived_at_ms: None,
                    save_label: None,
                    parent_session_id: None,
                    agent_label: None,
                    swarm_status: None,
                    edit_stats: None,
                    context: None,
                });
            }
        }
        if let Update::Sessions { sessions } = &mut update {
            self.live_swarm_status.merge(sessions);
            if same_session_catalog(&self.sessions, sessions) {
                // Keep exact recency metadata for future changes without
                // redrawing a catalog whose rows and order are unchanged.
                std::mem::swap(&mut self.sessions, sessions);
                return false;
            }
        }
        if let Update::WorkerFacts { workers } = &mut update {
            self.live_swarm_status.merge_worker_routes(workers);
            if workers == &self.worker_meta {
                return false;
            }
        }

        match update {
            Update::DraftModels { current, routes } => {
                Panel::publish_draft_models(current, routes, cx);
                for slot in &self.slots {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.seed_draft_models(cx));
                }
            }
            Update::Status(status) => {
                if self.status == status {
                    return false;
                }
                self.status = status;
            }
            Update::RemoteStatus {
                host,
                message,
                request_id,
                failed,
                failure,
            } => {
                if failed {
                    self.invalidate_cloud_connection(&host);
                }
                let label = remotes::machine_label(&host);
                let key = remotes::new_session_host(&host).to_owned();
                // A typed paired failure carries its own one-line sentence. Other
                // failures show the runtime's message, prefixed by the machine.
                let (line, notice) = match (&failure, failed) {
                    (Some(failure), true) => {
                        let summary = failure.summary(&label);
                        let notice = remotes::RowNotice {
                            summary: summary.clone(),
                            action: Some(failure.action()),
                            detail: Some(failure.detail.clone()),
                        };
                        (summary, Some(notice))
                    }
                    (None, true) => {
                        let raw = message.strip_prefix("Remote session failed: ").unwrap_or(&message);
                        let notice = remotes::RowNotice {
                            summary: crate::friendly_error::summarize(raw),
                            action: Some(crate::connect_failure::FailureAction::Retry),
                            detail: crate::friendly_error::summary_hides_details(raw)
                                .then(|| raw.to_owned()),
                        };
                        (format!("{label}: {message}"), Some(notice))
                    }
                    _ => (format!("{label}: {message}"), None),
                };
                self.update_pending_remote_status(request_id.as_deref(), &line, failed, cx);
                self.remotes.failed = failed;
                self.remotes.status = Some(line);
                self.remotes.status_host = Some(key.clone());
                match notice {
                    Some(notice) => {
                        self.remotes.reachable.remove(&key);
                        self.remotes.row_notices.insert(key, notice);
                    }
                    None => {
                        self.remotes.row_notices.remove(&key);
                        if message.starts_with("Connected") {
                            self.remotes.reachable.insert(key);
                        } else {
                            self.remotes.reachable.remove(&key);
                        }
                    }
                }
            }
            Update::RemoteSessions { request_id, result } => {
                self.apply_machine_sessions(request_id, result);
            }
            Update::SessionSearch {
                query,
                generation,
                result,
                elapsed_ms,
            } => self.apply_session_search(query, generation, result, elapsed_ms, cx),
            Update::Connected => {
                self.connected = true;
                self.status = "connected".into();
                if let Some(state) = self.resume.as_mut() {
                    state.start_on_close = true;
                } else if self.slots.get(self.active).is_some_and(|slot| {
                    slot.panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_startup_draft)
                }) {
                    self.start_default_startup(cx);
                }
            }
            Update::WorkerFacts { workers } => {
                self.worker_meta = workers;
                // Rows read the records, so the strip and the sidebar rebuild;
                // panels take the model their own record names when the
                // runtime has not sent an identity yet.
                self.sync_swarm_strips(cx);
            }
            Update::CatalogTodos(todos) => {
                let progress_changed = self.sidebar_todo_progress != todos.progress;
                let catalog = crate::applet_view::CatalogTasks {
                    rows: std::rc::Rc::new(todos.rows),
                    progress: std::rc::Rc::new(todos.progress.clone()),
                };
                if cx.try_global::<crate::applet_view::CatalogTasks>() != Some(&catalog) {
                    cx.set_global(catalog.clone());
                }
                if self.catalog_tasks != catalog {
                    // Another window may already have written the global.
                    // Compare with what this window last displayed.
                    for slot in &self.slots {
                        let Some(panel) = slot.panel.read_chat(cx) else {
                            continue;
                        };
                        let id = &panel.session_id;
                        if self.catalog_tasks.session(id) != catalog.session(id)
                            && panel.uses_catalog_todos()
                        {
                            App::notify(cx, slot.panel.entity_id());
                        }
                    }
                    self.catalog_tasks = catalog;
                }
                self.sidebar_todo_progress = todos.progress;
                if self.unfinished_sessions == todos.unfinished {
                    return progress_changed;
                }
                self.unfinished_sessions = todos.unfinished;
                crate::example_prompts::set(
                    crate::example_prompts::from_unfinished(&self.unfinished_sessions),
                    cx,
                );
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == "unfinished-work" {
                        let unfinished = self.unfinished_sessions.clone();
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.set_unfinished_work(unfinished, cx));
                    }
                }
                return true;
            }
            Update::Sessions { sessions } => {
                if self.pinned_working_dir.is_none()
                    && let Some(directory) = most_used_working_dir(&sessions)
                {
                    let persisted = directory.clone();
                    cx.background_executor()
                        .spawn(async move {
                            if let Err(error) =
                                crate::config::persist_pinned_working_dir(&persisted)
                            {
                                eprintln!("could not persist fixed directory shortcut: {error}");
                            }
                        })
                        .detach();
                    self.pinned_working_dir = Some(directory);
                }
                for session in &sessions {
                    for slot in &self.slots {
                        let Some(panel) = slot.panel.read_chat(cx) else {
                            continue;
                        };
                        if panel.session_id != session.session_id {
                            continue;
                        }
                        let title = session
                            .title
                            .as_ref()
                            .filter(|title| panel.title.as_ref() != *title);
                        // Restored panels can predate directory metadata. Refresh it
                        // along with the title so the identity footer stays current.
                        // Missing catalog metadata must not erase a known directory.
                        let directory = session.working_dir.as_ref().filter(|dir| {
                            !dir.trim().is_empty() && panel.working_dir.as_ref() != Some(*dir)
                        });
                        if title.is_some() || directory.is_some() {
                            slot.panel.update_chat(cx, |panel, cx| {
                                if let Some(title) = title {
                                    panel.title = title.clone().into();
                                }
                                if let Some(directory) = directory {
                                    panel.working_dir = Some(directory.clone());
                                }
                                cx.notify();
                            });
                        }
                        break;
                    }
                }
                self.sessions = sessions;
                self.sync_swarm_strips(cx);
                if !cfg!(test) {
                    // Swarm cards show each named worker's own model and total
                    // spend, read from its session files off the UI thread.
                    let wanted = self
                        .slots
                        .iter()
                        .filter_map(|slot| {
                            let panel = slot.panel.chat()?;
                            Some((panel.clone(), panel.read(cx).swarm_card_worker_ids()))
                        })
                        .filter(|(_, ids)| !ids.is_empty())
                        .collect::<Vec<_>>();
                    if !wanted.is_empty()
                        && let Some(home) = crate::harness::jcode_home()
                    {
                        let ids = wanted
                            .iter()
                            .flat_map(|(_, ids)| ids.clone())
                            .collect::<Vec<_>>();
                        cx.spawn(async move |_, cx| {
                            let usage = cx
                                .background_executor()
                                .spawn(async move {
                                    jcode_desktop_harness::swarm_usage::usage_for(&home, &ids)
                                })
                                .await;
                            for (panel, ids) in wanted {
                                let own = ids
                                    .iter()
                                    .filter_map(|id| Some((id.clone(), usage.get(id)?.clone())))
                                    .collect();
                                panel.update(cx, |panel, cx| panel.set_swarm_usage(own, cx));
                            }
                        })
                        .detach();
                    }
                }
            }
            Update::SessionCreated {
                session,
                request_id,
            } => {
                if request_id
                    .as_deref()
                    .is_some_and(Panel::is_pending_session_id)
                {
                    if let Some(slot) = self.slots.iter().find(|slot| {
                        !slot.closing && Some(slot.panel.session_id(cx)) == request_id.as_deref()
                    }) {
                        let session_id = session.session_id.clone();
                        let working_dir = session.working_dir.clone();
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.attach_startup_session(session, cx));
                        if let Some(request_id) = request_id.as_deref() {
                            self.resolve_collection_draft(request_id, &session_id, cx);
                        }
                        if request_id
                            .as_deref()
                            .is_some_and(crate::publish::is_publish_draft)
                        {
                            self.start_publish_session(session_id, working_dir.as_deref());
                        } else if request_id
                            .as_deref()
                            .is_some_and(|id| id.starts_with("startup://draft/help/"))
                        {
                            self.bridge.send(Command::Send {
                                session_id,
                                content: HELP_SESSION_PROMPT.into(),
                                images: Vec::new(),
                                attachments: Vec::new(),
                            });
                        }
                    } else {
                        // The user closed the draft during startup. Do not
                        // reopen it or steal focus when creation finishes.
                        self.pending_collection_drafts
                            .retain(|(draft, _)| Some(draft.as_str()) != request_id.as_deref());
                        self.bridge.send(Command::Unwatch {
                            session_id: session.session_id,
                        });
                    }
                    return true;
                }
                // A brand-new panel is only a local draft until its first prompt.
                // The persisted/runtime session refresh adds it after activity.
                let inserted = self.open_session(session, cx);
                self.set_active(inserted, cx);
                self.focus_pending = true;
            }
            Update::SessionForked { session, prompt } => {
                let session_id = session.session_id.clone();
                if !self
                    .sessions
                    .iter()
                    .any(|known| known.session_id == session_id)
                {
                    self.sessions.push(session.clone());
                }
                if self.single_panel {
                    self.open_fork_window(&session_id);
                    return true;
                }
                let inserted = self.open_session(session, cx);
                self.set_active(inserted, cx);
                self.focus_pending = true;
                // The harness already delivered the prompt over the child's
                // connection. Paint it here so the fork never opens looking
                // empty while the answer streams in.
                if let Some(prompt) = prompt {
                    self.slots[inserted]
                        .panel
                        .update_chat(cx, |panel, cx| panel.echo_external_prompt(prompt, cx));
                }
            }
            Update::SideQuestionStarted {
                parent_session_id,
                session,
                question,
            } => {
                // A side question answers inside the source chat's card. It is
                // never a tab of its own and never touches the source session.
                if let Some(index) = self.slots.iter().position(|slot| {
                    !slot.closing && slot.panel.session_id(cx) == parent_session_id
                }) {
                    self.slots[index].panel.update_chat(cx, |panel, cx| {
                        panel.start_side_question(session, question, cx)
                    });
                }
            }
            Update::SideQuestionHistory {
                parent_session_id,
                request_id,
                result,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| !slot.closing && slot.panel.session_id(cx) == parent_session_id)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.load_side_question_history(request_id, result, cx)
                    });
                }
            }
            Update::SideQuestionOpened {
                parent_session_id,
                request_id,
                result,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| !slot.closing && slot.panel.session_id(cx) == parent_session_id)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.load_saved_side_question(request_id, result, cx)
                    });
                }
            }
            Update::History {
                session_id,
                messages,
                images,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.load_history(messages, images, cx);
                        });
                        break;
                    }
                }
            }
            Update::HistoryPage { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_older_history(result, cx));
                        break;
                    }
                }
            }
            Update::BoundedHistory { session_id, page } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_bounded_history(page, cx));
                        break;
                    }
                }
            }
            Update::RewoundHistory {
                session_id,
                page,
                bounded,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.replace_rewound_history(page, bounded, cx)
                        });
                        break;
                    }
                }
            }
            Update::BoundedHistoryPage { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.load_bounded_history_page(result, cx)
                        });
                        break;
                    }
                }
            }
            Update::HistoryImage {
                session_id,
                key,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_history_image(key, result, cx));
                        break;
                    }
                }
            }
            Update::Event { session_id, event } => {
                if let jcode_sdk::ApiEvent::SwarmStatus {
                    session_id: source,
                    updated_at_unix_ms,
                    members,
                } = &event
                {
                    if self
                        .live_swarm_status
                        .apply(source, *updated_at_unix_ms, members)
                    {
                        let unknown_worker = members.iter().any(|member| {
                            member.parent_session_id.is_some()
                                && !self
                                    .sessions
                                    .iter()
                                    .any(|session| session.session_id == member.session_id)
                        });
                        self.live_swarm_status.merge(&mut self.sessions);
                        self.live_swarm_status.merge_worker_routes(&mut self.worker_meta);
                        self.sync_swarm_strips(cx);
                        if unknown_worker {
                            self.bridge.send(Command::RefreshSessions);
                        }
                        return true;
                    }
                    return false;
                }
                if self.route_side_question_event(&session_id, &event, cx) {
                    return false;
                }
                if let jcode_sdk::ApiEvent::FocusRequested { .. } = &event {
                    // Another session (an agent) asked the user to look at this
                    // one: select its tab and bring the app forward.
                    return self.raise_session(&session_id, cx);
                }
                if let jcode_sdk::ApiEvent::SidePanelState { snapshot, .. } = &event {
                    return self.apply_side_panel(&session_id, snapshot, cx);
                }
                if let jcode_sdk::ApiEvent::AppletState { snapshot, .. } = &event {
                    // Keyed by the panel's session id, which for remote
                    // hosts is namespaced, so instances land in that panel.
                    return crate::applet_runtime::get(cx).sync_agent(&session_id, snapshot);
                }
                self.forward_tool_call(&session_id, &event, cx);
                if sidebar_edits::refresh_after(&event) {
                    self.bridge.send(Command::RefreshSessions);
                }
                let mut changed = false;
                if matches!(event, jcode_sdk::ApiEvent::MessageAccepted { .. }
                    | jcode_sdk::ApiEvent::PeerUserMessage { .. }
                    | jcode_sdk::ApiEvent::ToolDone { .. }
                    | jcode_sdk::ApiEvent::TurnDone { .. }
                    | jcode_sdk::ApiEvent::TurnStopped { .. })
                    && let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|session| session.session_id == session_id)
                {
                    // Keep active work near the top like the TUI picker, but
                    // use message, tool and turn boundaries instead of tokens.
                    let now = unix_now_ms();
                    if session.updated_at_ms.is_none_or(|previous| previous < now) {
                        session.updated_at_ms = Some(now);
                        changed = true;
                    }
                }
                let updated_title = match &event {
                    jcode_sdk::ApiEvent::SessionRenamed { display_title, .. } => {
                        Some(display_title.clone())
                    }
                    jcode_sdk::ApiEvent::ToolDone { name, error, .. }
                        if name == "todo" && error.is_none() =>
                    {
                        // Todo-derived titles are SDK session metadata rather
                        // than explicit RenameSession events. Refresh the list
                        // as soon as the SDK reports a successful todo write.
                        self.bridge.send(Command::RefreshSessions);
                        None
                    }
                    _ => None,
                };
                if let Some(display_title) = updated_title
                    && let Some(session) = self
                        .sessions
                        .iter_mut()
                        .find(|session| session.session_id == session_id)
                    && session.title.as_ref() != Some(&display_title)
                {
                    // SessionInfo carries the effective display title, so keep
                    // the sidebar's cached copy in sync with the live panel.
                    session.title = Some(display_title);
                    changed = true;
                    if session.parent_session_id.is_some() {
                        self.sync_swarm_strips(cx);
                    }
                }
                let swarm_tool_finished =
                    matches!(&event, jcode_sdk::ApiEvent::ToolDone { name, .. } if name == "swarm");
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.apply(&event, cx);
                            if swarm_tool_finished {
                                panel.refresh_local_pending_awaits(cx);
                            }
                        });
                        if matches!(event, jcode_sdk::ApiEvent::TurnDone { .. })
                            && let Some(panel) = slot.panel.read_chat(cx)
                            && let Some(provider) = panel.provider.as_deref()
                        {
                            let id = accounts::credential_id(provider, panel.auth_method.as_deref());
                            if self.recent_accounts.first() != Some(&id) {
                                accounts::record_use(&mut self.recent_accounts, id);
                                changed = true;
                            }
                        }
                        break;
                    }
                }
                return changed;
            }
            Update::SendFailed { session_id, reason } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.message_failed(format!("message failed: {reason}"), cx);
                        });
                        break;
                    }
                }
            }
            Update::MessageSubmitted { .. } => {}
            Update::SessionSaved {
                session_id,
                saved,
                label,
            } => {
                let label = label
                    .map(|label| label.trim().to_string())
                    .filter(|label| !label.is_empty());
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|session| session.session_id == session_id)
                {
                    session.saved = saved;
                    if !saved {
                        session.save_label = None;
                    } else if let Some(label) = &label {
                        session.save_label = Some(label.clone());
                        session.title = Some(label.clone());
                    }
                }
                if saved && let Some(label) = &label {
                    for slot in &self.slots {
                        if slot.panel.session_id(cx) == session_id {
                            slot.panel.update_chat(cx, |panel, cx| {
                                panel.title = label.clone().into();
                                cx.notify();
                            });
                        }
                    }
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if panel.session_saved_settled(saved, label.as_deref(), None) {
                                cx.notify();
                            }
                        });
                    }
                }
                self.bridge.send(Command::RefreshSessions);
            }
            Update::SessionSaveFailed {
                session_id,
                saved,
                error,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if !panel.session_saved_settled(saved, None, Some(&error)) {
                                let verb = if saved { "save" } else { "unsave" };
                                panel.items.push(crate::panel::Item::Error(format!(
                                    "Failed to {verb} session: {error}"
                                )));
                            }
                            crate::sounds::play(crate::sounds::Cue::Error, cx);
                            cx.notify();
                        });
                        break;
                    }
                }
                // Undo the optimistic sidebar state from a pin click.
                self.bridge.send(Command::RefreshSessions);
            }
            Update::TodoState { session_id, state } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.todo_state_received(state, cx);
                        });
                        break;
                    }
                }
            }
            Update::TabWaitingState {
                session_id,
                waiting,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| slot.panel.session_id(cx) == session_id)
                {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.set_tab_waiting(waiting, cx));
                }
            }
            Update::EffortSettled {
                session_id,
                effort,
                error,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.effort_settled(&effort, error.as_deref(), cx);
                        });
                        break;
                    }
                }
            }
            Update::SpeedSettled { session_id, serial, result } => {
                if let Some(slot) = self.slots.iter().find(|slot| slot.panel.session_id(cx) == session_id) {
                    slot.panel.update_chat(cx, |panel, cx| panel.speed_settled(serial, result, cx));
                }
            }
            Update::AskAttachment {
                session_id,
                attachment_id,
                pending_ask_ids,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.reconcile_asks(attachment_id, pending_ask_ids.as_deref(), cx);
                        });
                    }
                }
            }
            Update::AskResponseSettled {
                session_id,
                request_id,
                attempt_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.ask_response_settled(&request_id, attempt_id, result.clone(), cx);
                        });
                    }
                }
            }
            Update::RoutingGateAnswerSettled {
                session_id,
                gate_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.routing_gate_answer_settled(&gate_id, result.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::RoutingIntentReply { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.routing_intent_reply(result.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::QueueMutationCapability {
                session_id,
                supported,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_mutations_supported(supported, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueueEditingCapability {
                session_id,
                supported,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_editing_supported(supported, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueuedPromptEdited {
                session_id,
                prompt_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queued_prompt_edited(&prompt_id, result.clone(), cx)
                        });
                        break;
                    }
                }
            }
            Update::QueueRemovalSettled {
                session_id,
                prompt,
                edit,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queue_removal_settled(prompt, edit, result, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueuePromptFailed {
                session_id,
                client_id,
                reason,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queue_prompt_failed(&client_id, reason.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::CommandFailed { session_id, reason } => {
                if self.route_side_question_event(
                    &session_id,
                    &jcode_sdk::ApiEvent::Error {
                        code: jcode_sdk::api::ErrorCode::Internal,
                        message: reason.clone(),
                    },
                    cx,
                ) {
                    return true;
                }
                if session_id == Panel::STARTUP_SESSION_ID {
                    self.remotes.startup_failed = true;
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if panel.is_pending_session() {
                                panel.status = "Session creation failed · draft retained".into();
                            }
                            panel.items.push(crate::panel::Item::Error(reason.clone()));
                            crate::sounds::play(crate::sounds::Cue::Error, cx);
                            cx.notify();
                        });
                        break;
                    }
                }
            }
            Update::Disconnected { reason } => {
                self.connected = false;
                self.status = format!(
                    "{} Reconnecting…",
                    crate::friendly_error::summarize(&reason)
                );
            }
            Update::SessionLost { session_id, reason } => {
                self.set_cloud_transport_connected(&session_id, false);
                if let Some(host) = harness::remote_host(&session_id) {
                    self.invalidate_cloud_connection(&host);
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_mutations_supported(false, cx);
                            panel.set_tab_waiting(false, cx);
                            panel.connection_lost(&reason, cx);
                        });
                        break;
                    }
                }
            }
            Update::SessionAttaching { session_id } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.status = "attaching…".into();
                            cx.notify();
                        });
                        break;
                    }
                }
            }
            Update::OpenrigHostIdentity {
                session_id,
                self_host_id,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_rig_host_id(self_host_id.clone(), cx);
                        });
                    }
                }
            }
            Update::SessionConnected { session_id } => {
                self.set_cloud_transport_connected(&session_id, true);
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            // A subsequent SessionStatus event will replace this
                            // with idle/running. Clear the stale lost banner now.
                            panel.status = "connected".into();
                            panel.refresh_local_pending_awaits(cx);
                            panel.prime_context_meter(cx);
                            cx.notify();
                        });
                        break;
                    }
                }
            }
        }
        true
    }

    fn wire_session_panel(panel: &Entity<Panel>, cx: &mut Context<Self>) {
        Panel::connect_input(panel, cx);
        if !cfg!(test) {
            panel.update(cx, |panel, cx| panel.refresh_local_pending_awaits(cx));
        }
        cx.subscribe(
            panel,
            |this, _, event: &crate::panel::OpenSwarmWorker, cx| {
                this.open_swarm_worker(&event.0, cx);
            },
        )
        .detach();
    }

    /// Open `session` as a panel immediately to the right of the focused panel.
    /// Returns the index of the new slot.
    fn open_session(&mut self, session: jcode_sdk::SessionInfo, cx: &mut Context<Self>) -> usize {
        self.open_session_in_row(session, self.active_row, cx)
    }

    /// Open `session` as a panel in `row`. A row other than the active one
    /// receives the panel at its end and stays hidden: focus, the active row
    /// and the camera are untouched.
    fn open_session_in_row(
        &mut self,
        session: jcode_sdk::SessionInfo,
        row: usize,
        cx: &mut Context<Self>,
    ) -> usize {
        self.open_session_with_identity(
            session.session_id,
            session.title,
            session.working_dir,
            row,
            cx,
        )
    }

    fn open_session_with_identity(
        &mut self,
        session_id: String,
        title: Option<String>,
        working_dir: Option<String>,
        row: usize,
        cx: &mut Context<Self>,
    ) -> usize {
        debug_assert!(row < STRIP_COUNT);
        let width_fraction = spawned_panel_width(
            self.row_indices(row)
                .filter(|&index| !self.slots[index].closing)
                .count(),
        );
        let bridge = self.bridge.clone();
        let is_startup = session_id == Panel::STARTUP_SESSION_ID;
        let panel = cx.new(|cx| Panel::new(session_id.clone(), title, working_dir, bridge, cx));
        Self::wire_session_panel(&panel, cx);
        if !Panel::is_pending_session_id(&session_id) {
            panel.update(cx, |panel, cx| panel.prefetch_persisted_history(cx));
            self.bridge.send(Command::Watch { session_id });
        }
        let slot = Slot {
            panel: PanelItem::Chat(panel),
            row,
            width_fraction,
            animated_width: AnimatedValue::new(
                0.0,
                transition::policy(Transition::PanelOpen).duration,
            ),
            order_offset: AnimatedValue::new(
                0.0,
                transition::policy(Transition::PanelOrder).duration,
            ),
            order_distance_fraction: width_fraction,
            close_progress: AnimatedValue::new(
                1.0,
                transition::policy(Transition::PanelClose).duration,
            ),
            closing: false,
            restore_fraction: None,
        };
        let active_is_on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == row);
        let row_last = self.row_indices(row).last();
        let insert_at = insert_index(self.active, active_is_on_strip, row_last, self.slots.len());
        self.slots.insert(insert_at, slot);
        self.slots[insert_at]
            .animated_width
            .set(width_fraction, Instant::now());
        // The lone panel that had the whole viewport steps back to the default
        // width now that it has company, leaving two equal halves.
        let live_indices = self
            .row_indices(row)
            .filter(|&index| !self.slots[index].closing)
            .collect::<Vec<_>>();
        let strip_panels = live_indices.len();
        let now = Instant::now();
        for index in live_indices {
            if index == insert_at {
                continue;
            }
            let slot = &mut self.slots[index];
            let demoted = demoted_width(slot.width_fraction, strip_panels);
            if demoted != slot.width_fraction {
                slot.width_fraction = demoted;
                slot.animated_width.set(demoted, now);
                slot.restore_fraction = None;
            }
        }
        // Inserting shifts every later index, including the focused one.
        if self.active >= insert_at {
            self.active += 1;
        }
        if !is_startup && row == self.active_row {
            crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        }
        insert_at
    }

    /// Push each coordinator panel its swarm workers for the composer
    /// indicator. Non-coordinators get none.
    fn sync_swarm_strips(&mut self, cx: &mut Context<Self>) {
        let now = swarm_now_ms();
        let workers =
            crate::swarm_view::workers_by_coordinator(&self.sessions, &self.worker_meta, now);
        let mut changed = false;
        for slot in self.slots.iter().filter(|slot| !slot.closing) {
            let Some(panel) = slot.panel.read_chat(cx) else {
                continue;
            };
            let list = workers
                .get(&panel.session_id)
                .map_or(&[][..], Vec::as_slice);
            let record = self.worker_meta.get(&panel.session_id);
            if !panel.swarm_strip_matches(list) || !panel.worker_record_matches(record) {
                changed = true;
                slot.panel
                    .update_chat(cx, |panel, cx| panel.sync_worker_strip(list, record, cx));
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn seed_tab_status_fixture(&self, cx: &mut Context<Self>) {
        use jcode_sdk::ApiEvent;
        for (index, slot) in self.slots.iter().take(4).enumerate() {
            slot.panel.update_chat(cx, |panel, cx| {
                let session_id = panel.session_id.clone();
                let status = |status: &str| ApiEvent::SessionStatus {
                    session_id: session_id.clone(),
                    status: status.into(),
                    pending_soft_interrupts: None,
                };
                match index {
                    0 => panel.apply(&status("running"), cx),
                    1 => panel.set_tab_waiting(true, cx),
                    2 => {
                        let event = serde_json::from_value(serde_json::json!({
                            "ev":"ask_request", "session_id":session_id,
                            "request_id":"fixture-ask", "tool_call_id":"fixture-tool",
                            "questions":[{"header":"Choice", "question":"Continue?", "options":[
                                {"label":"Yes", "description":"Continue the work"},
                                {"label":"No", "description":"Stop here"}
                            ]}]
                        }))
                        .expect("valid ask fixture");
                        panel.apply(&event, cx);
                    }
                    3 => {
                        panel.set_surface_focused(false, cx);
                        panel.apply(&status("running"), cx);
                        panel.apply(&status("idle"), cx);
                    }
                    _ => unreachable!(),
                }
            });
        }
    }

    /// Select an open session's tab and activate the app, so an agent that
    /// messages the user about another session can raise it. No-op when the
    /// session has no open panel here.
    fn raise_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.session_id(cx) == session_id)
        else {
            return false;
        };
        self.set_active(index, cx);
        self.overview = false;
        cx.activate(true);
        true
    }

    /// `JCODE_DESKTOP_SCREENSHOT_SWARM=real`: the user's real coordinator and
    /// its workers, from trimmed copies of their `~/.jcode` files (see
    /// `jcode-desktop-harness/src/fixtures`), so a screenshot shows real data
    /// shapes, not invented ones.
    fn load_real_swarm_fixture(&mut self, cx: &mut Context<Self>) {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../jcode-desktop-harness/src/fixtures");
        let home = fixtures.join("microbe_home");
        let mut sessions = harness::merge_persisted_sessions(Vec::new(), Some(&home));
        jcode_sdk::enrich_sessions_from_swarm_state(&mut sessions, home.join("state/swarm"));
        // The rows read each worker's record; the fixture homes already hold
        // trimmed copies of them, so read those rather than the real home. The
        // usage fixture keeps full token records for the workers it covers.
        let ids = jcode_sdk::worker_ids(&sessions);
        let mut meta = jcode_sdk::worker_meta_for(&home, &ids);
        meta.extend(jcode_sdk::worker_meta_for(
            &fixtures.join("usage_home"),
            &ids,
        ));
        self.worker_meta = meta;
        let Some(position) = sessions
            .iter()
            .position(|s| s.session_id.starts_with("session_microbe_"))
        else {
            return;
        };
        let coordinator = sessions.remove(position);
        sessions.insert(0, coordinator.clone());
        self.sessions = sessions;
        self.slots.clear();
        self.active = self.open_session(coordinator, cx);
        let ids = self.slots[self.active]
            .panel
            .read_chat(cx)
            .map(Panel::swarm_card_worker_ids)
            .unwrap_or_default();
        let usage =
            jcode_desktop_harness::swarm_usage::usage_for(&fixtures.join("usage_home"), &ids);
        self.slots[self.active]
            .panel
            .update_chat(cx, |panel, cx| panel.set_swarm_usage(usage, cx));
        self.sync_swarm_strips(cx);
    }

    /// Focus a worker's panel, opening it if needed (strip, sidebar, or DM).
    fn open_swarm_worker(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sidebar_closed_sessions.remove(session_id);
        let index = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.session_id(cx) == session_id)
            .unwrap_or_else(|| {
                let session = self
                    .sessions
                    .iter()
                    .find(|s| s.session_id == session_id)
                    .cloned();
                if let Some(session) = session {
                    self.open_session(session, cx)
                } else {
                    // A signed DM may arrive ahead of the next catalog refresh.
                    self.open_session_with_identity(
                        session_id.to_owned(),
                        None,
                        None,
                        self.active_row,
                        cx,
                    )
                }
            });
        self.set_active(index, cx);
        self.sync_swarm_strips(cx);
    }

    fn activate_session(
        &mut self,
        session: jcode_sdk::SessionInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == session.session_id)
            .unwrap_or_else(|| self.open_session(session, cx));
        // A sidebar card can be reopened before its close animation finishes.
        // Revive the original entity, including its composer draft, rather than
        // focusing a disappearing view or creating duplicate event recipients.
        if self.slots[index].closing {
            self.slots[index].closing = false;
            self.slots[index].close_progress.set(1.0, Instant::now());
            let session_id = self.slots[index].panel.session_id(cx).to_owned();
            if !Panel::is_pending_session_id(&session_id) {
                self.bridge.send(Command::Watch { session_id });
            }
        }
        self.set_active(index, cx);
        self.overview = false;
        self.overview_progress.set(0.0, Instant::now());
        self.focus_active(window, cx);
        cx.notify();
    }

    fn activate_unfinished_session(
        &mut self,
        unfinished: harness::UnfinishedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .sessions
            .iter()
            .find(|session| session.session_id == unfinished.session_id)
            .cloned()
            .unwrap_or_else(|| jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: unfinished.session_id,
                working_dir: unfinished.working_dir,
                title: Some(unfinished.title),
                status: "idle".into(),
                transcript_bytes: None,
                saved: false,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                save_label: None,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                edit_stats: None,
                context: None,
            });
        self.activate_session(session, window, cx);
    }

    /// Focus the slot at `index`, remembering the outgoing panel so
    /// `FocusPrevious` (niri's Alt+Tab) can return to it.
    fn set_active(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.slots.len() {
            return;
        }
        // Preserve the most recent chat when moving through several utility
        // panels, rather than relying on only the immediately previous slot.
        if let Some(slot) = self.slots.get(self.active)
            && slot.panel.read_chat(cx).is_some_and(Panel::supports_voice)
        {
            self.last_voice_chat = Some(slot.panel.entity_id());
        }
        if self.slots[index]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::supports_voice)
        {
            self.last_voice_chat = Some(self.slots[index].panel.entity_id());
        }
        let outgoing = self
            .slots
            .get(self.active)
            .filter(|_| self.active != index)
            .map(|slot| slot.panel.entity_id());
        if let Some(outgoing) = outgoing {
            self.previous = Some(outgoing);
        }
        if self.last_canvas_width.is_some()
            && (self.active != index || self.active_row != self.slots[index].row)
        {
            self.begin_map_navigation(self.slots[index].row);
        }
        self.compact_sidebar_open = false;
        self.active = index;
        self.active_row = self.slots[index].row;
        self.row_focus[self.active_row] = Some(self.slots[index].panel.entity_id());
        self.retarget_camera();
        cx.notify();
    }

    // --- Geometry -------------------------------------------------------

    fn slot_width(&self, index: usize, viewport: f32) -> f32 {
        let fraction = self.slots[index].width_fraction;
        Self::width_for_fraction(fraction, viewport)
    }

    fn width_for_fraction(fraction: f32, viewport: f32) -> f32 {
        // Narrow windows show one readable conversation without overwriting the
        // user's width presets. Widening restores the original arrangement.
        let fraction = if viewport < responsive::SINGLE_PANEL_BREAKPOINT {
            1.0
        } else {
            fraction
        };
        ((viewport - STRUT * 2.0) * fraction - GAP)
            .max(320.0_f32.min((viewport - STRUT * 2.0).max(1.0)))
    }

    fn slot_left(&self, index: usize, viewport: f32) -> f32 {
        let mut x = STRUT;
        for i in self.row_indices(self.slots[index].row) {
            if i == index {
                break;
            }
            x += self.slot_width(i, viewport) + GAP;
        }
        x
    }

    fn row_indices(&self, row: usize) -> impl Iterator<Item = usize> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| (slot.row == row).then_some(index))
    }

    fn active_position_in_row(&self) -> usize {
        self.row_indices(self.active_row)
            .position(|index| index == self.active)
            .unwrap_or(0)
    }

    fn select_row(&mut self, row: usize, preferred_position: usize) {
        self.begin_map_navigation(row.min(STRIP_COUNT - 1));
        self.active_row = row.min(STRIP_COUNT - 1);
        let remembered = self.row_focus[self.active_row].and_then(|entity_id| {
            self.slots
                .iter()
                .position(|slot| slot.row == self.active_row && slot.panel.entity_id() == entity_id)
        });
        let selected = remembered.or_else(|| {
            self.row_indices(self.active_row)
                .nth(preferred_position)
                .or_else(|| self.row_indices(self.active_row).last())
        });
        if let Some(index) = selected {
            if let Some(outgoing) = self
                .slots
                .get(self.active)
                .map(|slot| slot.panel.entity_id())
                && self.active != index
            {
                self.previous = Some(outgoing);
            }
            self.active = index;
            self.row_focus[self.active_row] = Some(self.slots[index].panel.entity_id());
        }
        self.retarget_camera();
    }

    fn retarget_camera(&mut self) {
        // Camera target is resolved during render when the viewport width is
        // known; setting a sentinel forces recomputation.
        self.camera_dirty[self.active_row] = true;
    }

    /// Resolve the strip's scroll offset. This follows niri's
    /// `center-focused-column "never"`: the camera only scrolls far enough to
    /// bring the focused panel fully on screen, keeping it at the left or right
    /// edge rather than centering it.
    fn resolve_camera_target(&mut self, viewport: f32) {
        self.camera_dirty[self.active_row] = false;
        // Target the settled layout, not the fading slot's old width. Closing
        // the left panel should pull its successor into place, not pan right
        // toward it and then reverse once the closing slot is removed.
        let surviving: Vec<_> = self
            .row_indices(self.active_row)
            .filter(|&index| !self.slots[index].closing)
            .collect();
        if surviving.is_empty() {
            self.camera_target[self.active_row] = 0.0;
            return;
        }
        let active = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
            .map(|_| self.active)
            .unwrap_or(surviving[0]);
        let left = STRUT
            + surviving
                .iter()
                .take_while(|&&index| index != active)
                .map(|&index| self.slot_width(index, viewport) + GAP)
                .sum::<f32>();
        let width = self.slot_width(active, viewport);
        let total = surviving
            .iter()
            .map(|&index| self.slot_width(index, viewport) + GAP)
            .sum::<f32>()
            + STRUT * 2.0;
        let current = self.camera_target[self.active_row];
        let target = scroll_into_view(current, left, width, viewport);
        let max_scroll = (total - viewport).max(-GAP);
        let target = target.clamp(-STRUT, max_scroll.max(-STRUT));
        let row = self.active_row;
        if (target - self.camera_target[row]).abs() > 0.01 {
            self.camera_from[row] = self.camera_x[row];
            self.camera_started[row] = Some(Instant::now());
            self.camera_target[row] = target;
        }
    }

    // --- Actions --------------------------------------------------------

    // --- Learning -------------------------------------------------------

    /// Record that the user reached an outcome by its keyboard shortcut.
    fn learned(&mut self, skill_id: &str, cx: &mut Context<Self>) {
        self.coach.used_shortcut(skill_id, learning::now());
        self.after_coach_update(cx);
    }

    /// Onboarding is complete after the user has successfully exercised every
    /// control it presents. `practiced` intentionally accepts prompted use: the
    /// full coach can continue building recall after the tutorial is complete.
    fn onboarding_complete(&self) -> bool {
        ONBOARDING_SKILLS
            .iter()
            .all(|skill| self.coach.trace(skill).practiced())
    }

    /// Record that the user reached the same outcome the long way, which is the
    /// evidence that they do not know (or have forgotten) the shortcut.
    fn missed(&mut self, skill_id: &str, cx: &mut Context<Self>) {
        self.coach.used_slow_path(skill_id, learning::now());
        self.after_coach_update(cx);
    }

    /// Reveal or retire the hint toast and persist the model when it changed.
    fn after_coach_update(&mut self, cx: &mut Context<Self>) {
        let now = learning::now();
        let hint = crate::config::coaching_hints()
            .then(|| self.coach.active_hint(now))
            .flatten();
        let visible = hint.is_some();
        if let Some(hint) = hint {
            self.coach_display_hint = Some(hint);
        }
        self.coach_progress
            .set(if visible { 1.0 } else { 0.0 }, Instant::now());
        if visible {
            self.coach_expiry_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(COACH_EXPIRY_WAKE).await;
                let _ = this.update(cx, |workspace, cx| workspace.after_coach_update(cx));
            }));
        } else {
            self.coach_expiry_task = None;
        }
        if self.coach.take_dirty()
            && let Some(persistence) = &self.learning_persistence
        {
            persistence.save(&self.coach);
        }
        cx.notify();
    }

    fn dismiss_coach_hint(&mut self, cx: &mut Context<Self>) {
        self.coach.dismiss_hint();
        self.coach_expiry_task = None;
        self.coach_progress.set(0.0, Instant::now());
        cx.notify();
    }

    /// Clicking a panel to focus it. Only counted as a slow path when the
    /// keyboard would genuinely have done the same job: clicking the already
    /// focused panel, or reaching into another strip, is not a missed shortcut.
    /// Distance matters too, since crossing a strip is what `super-end` is for.
    fn clicked_to_focus(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.active {
            return;
        }
        let Some(target_row) = self.slots.get(index).map(|slot| slot.row) else {
            return;
        };
        if target_row != self.active_row {
            // Another strip: the pointer is a reasonable way to get there.
            return;
        }
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        let from = indices.iter().position(|&i| i == self.active);
        let to = indices.iter().position(|&i| i == index);
        let Some((from, to)) = from.zip(to) else {
            return;
        };
        let steps = from.abs_diff(to);
        self.missed(click_skill(steps, to, indices.len()), cx);
    }

    fn focus_left(&mut self, _: &FocusLeft, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("H", false, "Focus left", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && let Some(&target) = indices[..position]
                .iter()
                .rev()
                .find(|&&index| !self.slots[index].closing)
        {
            self.set_active(target, cx);
            self.begin_action_capture("focus_left", window);
            self.focus_active(window, cx);
            // Credit only when the key did something: pressing into the edge of
            // a strip is a no-op and proves nothing either way.
            self.learned("focus_left_right", cx);
            cx.notify();
        }
    }

    fn focus_right(&mut self, _: &FocusRight, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("L", false, "Focus right", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && let Some(&target) = indices[position + 1..]
                .iter()
                .find(|&&index| !self.slots[index].closing)
        {
            self.set_active(target, cx);
            self.begin_action_capture("focus_right", window);
            self.focus_active(window, cx);
            self.learned("focus_left_right", cx);
            cx.notify();
        }
    }

    /// niri `focus-column-first`.
    fn focus_first(&mut self, _: &FocusFirst, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("Home", false, "Focus first panel", cx);
        let first = self.row_indices(self.active_row).next();
        if let Some(index) = first.filter(|index| *index != self.active) {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            self.learned("focus_first_last", cx);
            cx.notify();
        }
    }

    /// niri `focus-column-last`.
    fn focus_last(&mut self, _: &FocusLast, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("End", false, "Focus last panel", cx);
        let last = self.row_indices(self.active_row).last();
        if let Some(index) = last.filter(|index| *index != self.active) {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            self.learned("focus_first_last", cx);
            cx.notify();
        }
    }

    /// niri `focus-window-previous` (the user's Alt+Tab). Returns to the last
    /// focused panel wherever it now lives, including on another strip.
    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("Tab", false, "Return to previous panel", cx);
        let Some(previous) = self.previous else {
            return;
        };
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.entity_id() == previous)
        else {
            self.previous = None;
            return;
        };
        self.set_active(index, cx);
        self.focus_active(window, cx);
        self.learned("focus_previous", cx);
        cx.notify();
    }

    fn focus_up(&mut self, _: &FocusUp, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("K", false, "Focus strip above", cx);
        if self.active_row > 0 {
            self.change_row(self.active_row - 1, window, cx);
            // Empty strips still animate. Learning credit must not decide
            // whether an actual navigation is included in latency telemetry.
            self.begin_action_capture("focus_up", window);
        } else {
            // Pressing into the top edge cannot navigate, but the chord was
            // still produced by hand, so the tutorial lesson must not linger.
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
    }

    fn focus_down(&mut self, _: &FocusDown, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("J", false, "Focus strip below", cx);
        if self.active_row + 1 < STRIP_COUNT {
            self.change_row(self.active_row + 1, window, cx);
            self.begin_action_capture("focus_down", window);
        } else {
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
    }

    /// Move focus to another strip, crediting the shortcut only when the move
    /// was meaningful. Stepping onto an empty strip when there is nothing else
    /// open shows the user pressed a key, not that they navigated anywhere, so
    /// it must not be taken as evidence of skill. It is still hands-on
    /// practice, though: the tutorial asked for the chord and the user
    /// produced it, so the lesson must clear rather than linger forever on a
    /// workspace whose other strips happen to be empty.
    fn change_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let meaningful = self.switch_row_animated(row, window, cx);
        if meaningful {
            self.learned("focus_up_down", cx);
        } else {
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
        cx.notify();
        meaningful
    }

    fn begin_action_capture(&mut self, name: &'static str, window: &Window) {
        if let Some(capture) = self.action_capture.as_mut() {
            capture.begin(
                name,
                Instant::now(),
                window.frame_duration_snapshot(),
                window.input_latency_snapshot(),
            );
        }
    }

    /// Switch strips with the row transition animation. Returns whether the
    /// move actually landed on a different panel, so callers can decide if it
    /// counts as navigation.
    fn switch_row_animated(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = self.active_position_in_row();
        let was_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        self.select_row(row, position);
        self.focus_active(window, cx);
        let is_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let landed_somewhere = self.row_indices(self.active_row).next().is_some();
        landed_somewhere && was_active != is_active
    }

    fn move_panel_left(&mut self, _: &MovePanelLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("H", true, "Move panel left", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && position > 0
        {
            let previous = indices[position - 1];
            let moved_width = self.slots[self.active].width_fraction;
            let neighbour_width = self.slots[previous].width_fraction;
            self.slots.swap(self.active, previous);
            self.active = previous;
            self.start_order_animation(self.active, 1.0, neighbour_width);
            self.start_order_animation(indices[position], -1.0, moved_width);
            self.retarget_camera();
            self.learned("move_panel", cx);
            cx.notify();
        }
    }

    fn move_panel_right(&mut self, _: &MovePanelRight, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("L", true, "Move panel right", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && position + 1 < indices.len()
        {
            let next = indices[position + 1];
            let moved_width = self.slots[self.active].width_fraction;
            let neighbour_width = self.slots[next].width_fraction;
            self.slots.swap(self.active, next);
            self.active = next;
            self.start_order_animation(self.active, -1.0, neighbour_width);
            self.start_order_animation(indices[position], 1.0, moved_width);
            self.retarget_camera();
            self.learned("move_panel", cx);
            cx.notify();
        }
    }

    fn start_order_animation(&mut self, index: usize, offset: f32, distance_fraction: f32) {
        let duration = transition::policy(Transition::PanelOrder).duration;
        self.slots[index].order_offset = AnimatedValue::new(offset, duration);
        self.slots[index].order_offset.set(0.0, Instant::now());
        self.slots[index].order_distance_fraction = distance_fraction;
    }

    fn move_panel_up(&mut self, _: &MovePanelUp, window: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("K", true, "Move panel up", cx);
        self.move_panel_to_row(-1, window, cx);
    }

    /// niri `move-column-to-first`.
    fn move_panel_to_first(
        &mut self,
        _: &MovePanelToFirst,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.showcase_motion("Home", true, "Move panel to start", cx);
        let first = self.row_indices(self.active_row).next();
        let on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == self.active_row);
        let Some(first) = first.filter(|first| on_strip && *first != self.active) else {
            return;
        };
        let slot = self.slots.remove(self.active);
        self.slots.insert(first, slot);
        self.active = first;
        self.retarget_camera();
        self.learned("move_panel_end", cx);
        cx.notify();
    }

    /// niri `move-column-to-last`.
    fn move_panel_to_last(&mut self, _: &MovePanelToLast, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("End", true, "Move panel to end", cx);
        let last = self.row_indices(self.active_row).last();
        let on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == self.active_row);
        let Some(last) = last.filter(|last| on_strip && *last != self.active) else {
            return;
        };
        let slot = self.slots.remove(self.active);
        self.slots.insert(last, slot);
        self.active = last;
        self.retarget_camera();
        self.learned("move_panel_end", cx);
        cx.notify();
    }

    fn move_panel_down(&mut self, _: &MovePanelDown, window: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("J", true, "Move panel down", cx);
        self.move_panel_to_row(1, window, cx);
    }

    fn move_panel_to_row(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.active_row as isize + delta;
        if !(0..STRIP_COUNT as isize).contains(&target) {
            return;
        }
        if self
            .slots
            .get(self.active)
            .is_none_or(|slot| slot.row != self.active_row)
        {
            return;
        }
        self.begin_map_navigation(target as usize);
        let slot = &mut self.slots[self.active];
        slot.row = target as usize;
        self.active_row = target as usize;
        self.row_focus[self.active_row] = Some(slot.panel.entity_id());
        self.retarget_camera();
        self.focus_active(window, cx);
        self.learned("move_panel_strip", cx);
        cx.notify();
    }

    fn new_panel(&mut self, _: &NewPanel, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_panel {
            self.open_new_session_window(cx);
            return;
        }
        self.tutorial_cue("Enter", "New session", "new", cx);
        self.learned("new_panel", cx);
        self.open_new_session(cx);
    }

    fn fork_panel(&mut self, _: &ForkPanel, _: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.slots.get(self.active).and_then(|slot| {
            let panel = slot.panel.read_chat(cx)?;
            (panel.preview_state.is_none() && panel.can_fork()).then(|| panel.session_id.clone())
        }) else {
            return;
        };
        self.bridge.send(Command::Fork { session_id });
    }

    fn new_panel_in_pinned_directory(
        &mut self,
        _: &NewPanelInPinnedDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_panel(&NewPanel, window, cx);
    }

    fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        let working_dir = self.default_working_dir();
        if self.single_panel {
            if let Err(error) = panel_window::open_terminal_window(working_dir, self.host, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let host = self.host;
        let terminal = cx.new(|cx| TerminalPanel::new(working_dir, None, None, host, cx));
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Terminal(terminal),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn close_accounts(
        &mut self,
        panel: &Entity<Panel>,
        source: &Entity<Panel>,
        choose_model: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.chat() == Some(panel))
        else {
            return;
        };
        self.set_active(index, cx);
        self.close_panel(&ClosePanel, window, cx);
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.chat() == Some(source))
        {
            self.set_active(index, cx);
            if source.read(cx).can_refresh_account_runtime() {
                self.bridge.send(Command::RefreshRuntime {
                    session_id: source.read(cx).session_id.clone(),
                });
            }
            if choose_model {
                source.update(cx, |panel, cx| panel.choose_account_model(cx));
            }
            self.retarget_camera();
            self.focus_active(window, cx);
        }
    }

    fn open_model_window_or_picker(
        &mut self,
        request: &OpenModelPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self
            .slots
            .iter()
            .find(|slot| !slot.closing && slot.panel.entity_id() == request.source)
            .and_then(|slot| slot.panel.chat().cloned())
        else {
            return;
        };
        source.update(cx, |panel, cx| panel.toggle_model_picker(window, cx));
    }

    fn open_accounts(
        &mut self,
        request: &OpenAccounts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source_index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.entity_id() == request.source)
        else {
            return;
        };
        let Some(source) = self.slots[source_index].panel.chat().cloned() else {
            return;
        };
        if source.read(cx).is_accounts_panel() {
            if let Some(command) = &request.login_command {
                source.update(cx, |panel, cx| panel.login_command(command, cx));
            }
            self.set_active(source_index, cx);
            self.retarget_camera();
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let source_session = source.read(cx).session_id.clone();
        if self.single_panel {
            let preview = source.read(cx).preview_state;
            let panel =
                cx.new(|cx| Panel::new_accounts(&source_session, preview, self.bridge.clone(), cx));
            match panel_window::open_panel_window(panel, Some(source), window, cx) {
                Ok(panel) => {
                    if let Some(command) = &request.login_command {
                        panel.update(cx, |panel, cx| {
                            panel.login_command(command, cx);
                        });
                    }
                }
                Err(error) => eprintln!("could not open Accounts window: {error:#}"),
            }
            return;
        }
        if let Some(index) = self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.session_id(cx) == format!("accounts://{source_session}")
        }) {
            if let Some(command) = &request.login_command {
                self.slots[index].panel.update_chat(cx, |panel, cx| {
                    panel.login_command(command, cx);
                });
            }
            self.set_active(index, cx);
            self.retarget_camera();
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let row = self.slots[source_index].row;
        let preview = source.read(cx).preview_state;
        let panel =
            cx.new(|cx| Panel::new_accounts(&source_session, preview, self.bridge.clone(), cx));
        if let Some(command) = &request.login_command {
            panel.update(cx, |panel, cx| {
                panel.login_command(command, cx);
            });
        }
        let source_for_close = source.clone();
        cx.subscribe_in(
            &panel,
            window,
            move |this, _, event: &crate::panel::AccountsPanelRedeemReset, window, cx| {
                this.open_usage_reset(&event.0, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &panel,
            window,
            move |this, panel, _: &crate::panel::AccountsPanelClosed, window, cx| {
                this.close_accounts(panel, &source_for_close, false, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &panel,
            window,
            move |this, panel, _: &crate::panel::AccountsPanelChooseModel, window, cx| {
                this.close_accounts(panel, &source, true, window, cx);
            },
        )
        .detach();
        let width_fraction = spawned_panel_width(self.slots.len());
        let insert_at = source_index + 1;
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        if self.active >= insert_at {
            self.active += 1;
        }
        let row_count = self
            .slots
            .iter()
            .filter(|slot| slot.row == row && !slot.closing)
            .count();
        let source = &mut self.slots[source_index];
        let width = demoted_width(source.width_fraction, row_count);
        if width != source.width_fraction {
            source.width_fraction = width;
            source.animated_width.set(width, Instant::now());
            source.restore_fraction = None;
        }
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Super+Shift+G and the sidebar inbox entry open the bundled Gmail
    /// applet. An open Gmail panel is focused instead of duplicated.
    fn open_gmail(&mut self, _: &OpenGmail, window: &mut Window, cx: &mut Context<Self>) {
        let prefix = format!(
            "{}{GMAIL_APPLET}#",
            crate::panel::applet_panel::APPLET_PREFIX
        );
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.session_id(cx).starts_with(&prefix))
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let launched = crate::applet_runtime::get(cx)
            .launch_trigger(GMAIL_APPLET, jcode_applet_types::manifest::Trigger::Sidebar);
        match launched {
            Ok(Some(instance)) => self.open_applet_instance(&instance, window, cx),
            // The provider mounts shortly and the applet loop opens its panel.
            Ok(None) => {}
            Err(error) => {
                eprintln!("jcode desktop: {error}");
                self.applet_toast =
                    Some((format!("Could not open Gmail: {error}"), Instant::now()));
            }
        }
        cx.notify();
    }

    fn open_code_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_document_file(
            &crate::ui_actions::OpenDocumentFile {
                path,
                fragment: None,
            },
            window,
            cx,
        );
    }

    fn open_document_file(
        &mut self,
        action: &crate::ui_actions::OpenDocumentFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let requested = action.path.clone();
        let fragment = action.fragment.clone();
        cx.spawn_in(window, async move |this, cx| {
            let path = cx
                .background_executor()
                .spawn(async move { requested.canonicalize().unwrap_or(requested) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_resolved_document_file(path, fragment, window, cx);
            });
        })
        .detach();
    }

    fn open_resolved_document_file(
        &mut self,
        path: PathBuf,
        fragment: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let show_heading = |file: &Entity<FilePanel>, cx: &mut App| {
            if let Some(fragment) = &fragment {
                file.update(cx, |file, cx| file.show_heading(fragment, cx));
            }
        };
        let session_id = FilePanel::session_id_for(&path);
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == session_id)
        {
            if let Some(file) = self.slots[index].panel.file() {
                show_heading(file, cx);
            }
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }

        let width_fraction = spawned_panel_width(self.slots.len());
        let file = cx.new(|cx| FilePanel::new(path, cx));
        if self.single_panel {
            match panel_window::open_file_window(file, window, cx) {
                Ok(file) => show_heading(&file, cx),
                Err(error) => eprintln!("Could not open panel window: {error:#}"),
            }
            return;
        }
        show_heading(&file, cx);
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::File(file),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn new_unfinished_work(
        &mut self,
        _: &NewUnfinishedWork,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == "unfinished-work")
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let workspace = cx.weak_entity();
        let panel = cx.new(|cx| {
            Panel::new_unfinished_work(
                self.unfinished_sessions.clone(),
                std::sync::Arc::new(move |session, window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.activate_unfinished_session(session, window, cx);
                        })
                        .expect("unfinished-work workspace is still available");
                }),
                self.bridge.clone(),
                cx,
            )
        });
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn orchestration_opener(&self, cx: &mut Context<Self>) -> crate::panel::SessionOpener {
        let workspace = cx.weak_entity();
        std::sync::Arc::new(move |session, window, cx| {
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.activate_unfinished_session(session, window, cx);
            });
        })
    }

    /// One orchestration panel per workspace: live sessions, whether each is
    /// running, and their todos.
    fn open_orchestration(
        &mut self,
        _: &OpenOrchestration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_orchestration(None, window, cx);
    }

    fn show_orchestration(
        &mut self,
        target: Option<jcode_sdk::SessionInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = target.map(|session| {
            let source = self
                .slots
                .iter()
                .find(|slot| !slot.closing && slot.panel.session_id(cx) == session.session_id);
            (session, source.and_then(|slot| slot.panel.chat().cloned()))
        });
        if let Some(index) = self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.session_id(cx) == crate::panel::orchestration::SESSION_ID
        }) {
            self.slots[index]
                .panel
                .update_chat(cx, |panel, cx| panel.set_orchestration_focus(focus, cx));
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let opener = self.orchestration_opener(cx);
        let panel = cx.new(|cx| Panel::new_orchestration(opener, self.bridge.clone(), cx));
        if self.single_panel {
            match panel_window::open_panel_window(panel, None, window, cx) {
                Ok(panel) => panel.update(cx, |panel, cx| panel.set_orchestration_focus(focus, cx)),
                Err(error) => eprintln!("Could not open orchestration window: {error:#}"),
            }
        } else {
            panel.update(cx, |panel, cx| panel.set_orchestration_focus(focus, cx));
            self.insert_spawned_panel(panel, window, cx);
        }
    }

    fn open_openrig(&mut self, _: &OpenOpenrig, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == crate::panel::openrig::SESSION_ID)
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let panel = cx.new(|cx| Panel::new_openrig(self.bridge.clone(), cx));
        self.insert_spawned_panel(panel, window, cx);
    }

    fn insert_spawned_panel(
        &mut self,
        panel: Entity<Panel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOrder).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn open_todoist(&mut self, _: &OpenTodoist, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == "todoist://tasks")
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let panel = cx.new(|cx| Panel::new_todoist(self.bridge.clone(), cx));
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row: self.active_row,
                width_fraction,
                animated_width: AnimatedValue::new(
                    width_fraction,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_offset: AnimatedValue::new(
                    0.0,
                    transition::policy(Transition::PanelOpen).duration,
                ),
                order_distance_fraction: width_fraction,
                close_progress: AnimatedValue::new(
                    1.0,
                    transition::policy(Transition::PanelClose).duration,
                ),
                closing: false,
                restore_fraction: None,
            },
        );
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Open a session without attributing the choice to the keyboard. Pointer
    /// paths call this directly, so clicking never earns keyboard credit.
    fn open_new_session(&mut self, cx: &mut Context<Self>) {
        self.open_default_draft(self.new_session_directory(), cx);
    }

    fn close_panel(&mut self, _: &ClosePanel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(slot) = self.slots.get(self.active).filter(|slot| !slot.closing)
            && !slot.panel.confirm_close(cx)
        {
            cx.stop_propagation();
            return;
        }
        if self.single_panel && self.active == 0 {
            // A standalone chat has no empty workspace to return to.
            window.remove_window();
            return;
        }
        self.tutorial_cue("Q", "Close panel", "close", cx);
        if self
            .slots
            .get(self.active)
            .is_none_or(|slot| slot.row != self.active_row || slot.closing)
        {
            return;
        }
        if self.slots[self.active]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::is_default_directory)
        {
            self.close_default_directory_picker(cx);
            self.focus_active(window, cx);
            return;
        }
        if self.slots[self.active]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::is_machines)
        {
            self.close_machines(cx);
            self.focus_active(window, cx);
            return;
        }
        self.learned("close_panel", cx);
        crate::sounds::play(crate::sounds::Cue::PanelClose, cx);
        let closed = self.active;
        let closed_id = self.slots[closed].panel.entity_id();
        self.slots[closed].closing = true;
        self.slots[closed].close_progress.set(0.0, Instant::now());
        let session_id = self.slots[closed].panel.session_id(cx).to_owned();
        // An open queue edit must not keep the daemon from delivering it.
        self.slots[closed]
            .panel
            .update_chat(cx, |panel, _| panel.release_queue_edit_hold());
        // Only a runtime conversation has a session to stop watching.
        let watched = self.slots[closed].panel.read_chat(cx).is_some_and(|panel| {
            !panel.is_pending_session()
                && !panel.is_side_document()
                && !panel.is_change_review()
                && !panel.is_accounts_panel()
                && !panel.is_machines()
                && panel.preview_state.is_none()
        });
        if watched {
            self.sidebar_closed_sessions.insert(session_id.clone());
            self.bridge.send(Command::Unwatch { session_id });
        }
        if self.previous == Some(closed_id) {
            self.previous = None;
        }
        let remaining: Vec<_> = self
            .row_indices(self.active_row)
            // Dismissed slots remain mounted for their fade. In particular,
            // closing from the right must not return to the fading neighbour
            // on the next key repeat.
            .filter(|&index| !self.slots[index].closing)
            .collect();
        // A fading neighbour no longer owns canvas space. Restore the lone
        // ordinary column now, rather than leaving half the window empty after
        // the fade completes. Non-default widths remain the user's choice.
        if let [index] = remaining.as_slice() {
            let slot = &mut self.slots[*index];
            if slot.width_fraction == DEFAULT_WIDTH {
                slot.width_fraction = 1.0;
                slot.animated_width.set(1.0, Instant::now());
                slot.restore_fraction = None;
            }
        }
        self.active = if self.single_panel {
            0
        } else if self.slots[closed].panel.is_changelog() {
            self.previous
                .and_then(|id| {
                    remaining
                        .iter()
                        .copied()
                        .find(|&index| self.slots[index].panel.entity_id() == id)
                })
                .unwrap_or_else(|| focus_after_close(closed, &remaining))
        } else {
            focus_after_close(closed, &remaining)
        };
        self.row_focus[self.active_row] = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
            .map(|slot| slot.panel.entity_id());
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn remove_finished_closing_panels(
        &mut self,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let mut finished = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let progress = slot.close_progress.sample(now);
            if slot.closing && !slot.close_progress.is_animating() && progress <= f32::EPSILON {
                finished.push(index);
            }
        }
        if finished.is_empty() {
            return;
        }
        for index in finished.into_iter().rev() {
            self.slots.remove(index);
        }
        self.active = active_id
            .and_then(|id| {
                self.slots
                    .iter()
                    .position(|slot| slot.panel.entity_id() == id)
            })
            .unwrap_or_else(|| self.slots.len().saturating_sub(1));
        if self.slots.is_empty() {
            window.focus(&self.focus_handle, cx);
        }
        self.retarget_camera();
    }

    fn toggle_overview(&mut self, _: &ToggleOverview, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_hints(&mut self, _: &ToggleHints, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("/", "Show all shortcuts", "help", cx);
        self.hints_overlay = !self.hints_overlay;
        self.hints_progress
            .set(if self.hints_overlay { 1.0 } else { 0.0 }, Instant::now());
        cx.notify();
    }

    fn toggle_showcase(&mut self, _: &ToggleShowcase, _: &mut Window, cx: &mut Context<Self>) {
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

    fn showcase_motion(
        &mut self,
        key: &str,
        shifted: bool,
        action: &'static str,
        cx: &mut Context<Self>,
    ) {
        if !self.showcase_mode {
            return;
        }
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        };
        self.showcase_cue = Some(ShowcaseCue {
            started_at: Some(Instant::now()),
            shortcut: if shifted {
                format!("{modifier} + Shift + {key}")
            } else {
                format!("{modifier} + {key}")
            },
            action,
            tutorial_group: if shifted { "move" } else { "navigate" },
        });
        self.schedule_showcase_expiry(cx);
        cx.notify();
    }

    fn tutorial_cue(
        &mut self,
        key: &str,
        action: &'static str,
        tutorial_group: &'static str,
        cx: &mut Context<Self>,
    ) {
        if !self.showcase_mode {
            return;
        }
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        };
        self.showcase_cue = Some(ShowcaseCue {
            started_at: Some(Instant::now()),
            shortcut: format!("{modifier} + {key}"),
            action,
            tutorial_group,
        });
        self.schedule_showcase_expiry(cx);
        cx.notify();
    }

    fn schedule_showcase_expiry(&mut self, cx: &mut Context<Self>) {
        if self.showcase_cue.is_none() {
            self.showcase_task = None;
            return;
        }
        self.showcase_task = Some(cx.spawn(async move |this, cx| {
            // Sleep through the readable hold, then wake rendering for the fade.
            // Replacing this task on every press cancels the previous expiry.
            cx.background_executor()
                .timer(SHOWCASE_DURATION - notifications::SHOWCASE_FADE)
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor()
                .timer(notifications::SHOWCASE_FADE)
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.showcase_cue = None;
                workspace.showcase_task = None;
                cx.notify();
            });
        }));
    }

    fn select_theme(&mut self, preset: ThemePreset, cx: &mut Context<Self>) {
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

    fn cycle_theme(&mut self, _: &CycleTheme, _: &mut Window, cx: &mut Context<Self>) {
        self.select_theme(Theme::active_preset().next(), cx);
    }

    fn new_help_session(&mut self, _: &NewHelpSession, _: &mut Window, cx: &mut Context<Self>) {
        self.hints_overlay = false;
        self.hints_progress.set(0.0, Instant::now());
        self.open_local_draft_kind(self.default_working_dir(), true, cx);
        cx.notify();
    }

    fn set_width(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let preset = match fraction {
            0.25 => Some(("1", "Panel width 25%")),
            0.5 => Some(("2", "Panel width 50%")),
            0.75 => Some(("3", "Panel width 75%")),
            1.0 => Some(("4", "Panel width 100%")),
            _ => None,
        };
        if let Some((key, action)) = preset {
            self.tutorial_cue(key, action, "resize", cx);
        }
        if let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        {
            slot.width_fraction = fraction;
            slot.animated_width.set(fraction, Instant::now());
            slot.restore_fraction = None;
            self.retarget_camera();
            self.learned("width_presets", cx);
        }
    }

    /// niri `switch-preset-column-width` (Alt+R): step to the next preset,
    /// wrapping around.
    fn cycle_width(&mut self, _: &CycleWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("R", "Cycle panel width", "resize", cx);
        let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        else {
            return;
        };
        slot.width_fraction = next_preset(slot.width_fraction);
        slot.animated_width.set(slot.width_fraction, Instant::now());
        slot.restore_fraction = None;
        self.retarget_camera();
        self.learned("cycle_width", cx);
    }

    /// niri `maximize-column` (Alt+F): fill the viewport, or restore the
    /// previous width when already maximized.
    fn maximize_width(&mut self, _: &MaximizeWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("F", "Maximize or restore panel", "resize", cx);
        let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        else {
            return;
        };
        let (width, restore) = toggle_maximize(slot.width_fraction, slot.restore_fraction);
        slot.width_fraction = width;
        slot.animated_width.set(width, Instant::now());
        slot.restore_fraction = restore;
        self.retarget_camera();
        self.learned("maximize", cx);
    }

    /// Keep the highlighted panel equal to the panel that receives typing.
    ///
    /// Keyboard focus can enter a panel without passing through `set_active`:
    /// a hot reload restores the focused composer by index, panel controls
    /// call `focus_input` and stop the click from reaching the surface, and a
    /// press inside one composer can be released over another panel. The
    /// reverse also happens: several paths call `set_active` and leave focus
    /// in the outgoing composer. Either way one session was highlighted while
    /// another took the prompt.
    ///
    /// Whichever side moved since the last frame is the user's intent. A new
    /// active panel pulls focus to it, otherwise the highlight follows focus.
    /// Focus held outside every panel (menus, search, the workspace) is left
    /// alone.
    fn follow_keyboard_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overview || self.overview_progress.is_animating() {
            return;
        }
        let active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let owner = self
            .slots
            .iter()
            .position(|slot| {
                if slot.closing { return false; }
                let panel = self.panel_metadata(slot, cx);
                panel.keyboard_focus.iter().flatten().any(|handle| handle.is_focused(window))
                    || panel.panel_focus.contains_focused(window, cx)
            });
        if let Some(index) = owner
            && index != self.active
        {
            if self.focus_synced_active == active {
                self.set_active(index, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        self.focus_synced_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
    }

    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        // Runtime updates must not move focus behind a launch-only modal.
        if self.account_sign_in.visible || self.onboarding_launch.error.is_some() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        // The session picker replaces every panel, so "the active panel" is
        // not on screen while it is open. Focus belongs to its query field.
        if let Some(state) = &self.resume {
            window.focus(&state.search.focus_handle(cx), cx);
            return;
        }
        // Overview cards contain copied content, not mounted panel views. Keep
        // shortcuts on the workspace until the exit animation remounts them.
        if self.overview || self.overview_progress.is_animating() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        if let Some(slot) = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
        {
            slot.panel.focus_input(window, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    // --- Rendering ------------------------------------------------------

    fn render_row(
        &mut self,
        row: usize,
        viewport_w: f32,
        viewport_h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.row_indices(row).next().is_some() {
            self.render_strip(row, viewport_w, viewport_h, window, cx)
        } else {
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap_2()
                .items_center()
                .justify_center()
                .text_color(Theme::global().TEXT_DIM)
                // Empty strips still allow an upward touchpad hop. Moving
                // down requires the keyboard shortcut, even on an empty row.
                .on_scroll_wheel(cx.listener(
                    move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                        if this.active_row != row || !event.delta.precise() {
                            return;
                        }
                        let delta = event.delta.pixel_delta(window.line_height());
                        let dy = f32::from(delta.y);
                        // Moving down a workspace requires an explicit shortcut.
                        // Ignore touch travel in that direction, including previews.
                        if dy < 0.0 {
                            this.gesture.pull = 0.0;
                            cx.notify();
                            return;
                        }
                        let now = cx.background_executor().now();
                        if this
                            .gesture_seen
                            .is_none_or(|seen| now.duration_since(seen) > GESTURE_RESET)
                        {
                            this.gesture.reset();
                        }
                        this.gesture_seen = Some(now);
                        let switch = this.gesture.feed_vertical(dy, event.touch_phase);
                        if switch != 0 {
                            let target = if switch > 0 {
                                (this.active_row + 1).min(STRIP_COUNT - 1)
                            } else {
                                this.active_row.saturating_sub(1)
                            };
                            if target != this.active_row {
                                this.switch_row_animated(target, window, cx);
                            }
                        }
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .debug_selector(|| "empty-strip-hint".into())
                        .text_color(Theme::global().TEXT)
                        .child(format!("Workspace {} is empty", row + 1)),
                )
                .child(
                    div()
                        .debug_selector(|| "empty-strip-shortcut".into())
                        .child("Press Super+Enter to start a new session here"),
                )
                .child(div().text_size(px(12.0)).child(self.status.clone()))
                .into_any_element()
        }
    }

    /// A one-line snapshot of the strip layout. Written to the path in
    /// `JCODE_DESKTOP_STATE` on every render so an automated check can observe
    /// what the running window is actually doing.
    fn dump_state(&self, window: &Window, cx: &mut Context<Self>) {
        if self.launch.var("JCODE_DESKTOP_STATE").is_none() {
            return;
        }
        // Written after the frame, outside rendering: reading every panel's
        // input here would make this view depend on all of them, and draw it
        // again whenever any caret blinks.
        let workspace = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = workspace.read_with(cx, |workspace, cx| workspace.write_state(window, cx));
        });
    }

    fn write_state(&self, window: &Window, cx: &App) {
        let Some(path) = self.launch.var("JCODE_DESKTOP_STATE").map(str::to_owned) else {
            return;
        };
        let widths: Vec<f32> = self
            .row_indices(self.active_row)
            .map(|index| self.slots[index].width_fraction)
            .collect();
        let focus = self
            .row_indices(self.active_row)
            .position(|index| index == self.active);
        let line = describe_strip(&widths, focus, self.active_row);
        // The coach's decisions are also dumped, so an automated check can see
        // what the running app believes the user knows and what it is teaching.
        let now = learning::now();
        let coach = describe_coach(
            self.coach.overall_mastery(now),
            self.coach.effort_saved,
            self.coach.effort_wasted,
            self.coach.active_hint_id(),
        );
        let performance = self.performance.as_ref().map(|profile| {
            let snapshot = profile.snapshot();
            let gpui = self.gpui_performance;
            format!(
                "perf animation_fps={:.1} frame_p95_ms={:.1} missed={} view_p95_ms={:.1} draw_p95_ms={:.1} present_p95_ms={:.1} input_to_frame_p95_ms={:.1} coalesced={} mid_draw={}",
                snapshot.animation_fps,
                snapshot.animation_frame_p95_ms,
                snapshot.missed_animation_frames,
                snapshot.render_p95_ms,
                gpui.draw_p95_ms,
                gpui.present_p95_ms,
                gpui.input_to_frame_p95_ms,
                gpui.input_events_per_frame_p95,
                gpui.mid_draw_inputs,
            )
        });
        let suffix = performance
            .map(|line| format!("\n{line}"))
            .unwrap_or_default();
        let appearance = format!(
            "layout={:?} sidebar={:?}",
            self.layout_mode, self.sidebar_view
        );
        let navigation = self.navigation_state(window, cx);
        let contents = format!("{line}\n{coach}\n{appearance}{suffix}\nnavigation={navigation}\n");
        // Frames mostly repeat the last state. Skipping identical writes keeps
        // the file syscalls out of the frame loop the harness is measuring.
        thread_local! {
            static LAST: std::cell::RefCell<(String, String)> = Default::default();
        }
        LAST.with_borrow_mut(|last| {
            if last.0 == path && last.1 == contents {
                return;
            }
            if std::fs::write(&path, &contents).is_ok() {
                *last = (path, contents);
            }
        });
    }

    /// Strip rendering normally advances these values. Overview and completed
    /// row switches omit hidden strips, but elapsed animations must still retire
    /// or animation_active will keep rearming the fallback frame timer forever.
    fn advance_hidden_strip_animations(&mut self, now: Instant, overview_visible: bool) {
        static STALE_FIXTURE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
            (harness::screenshot_mode() || cfg!(test))
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_STALE_HIDDEN_ANIMATIONS").as_deref()
                    == Ok("1")
        });
        if *STALE_FIXTURE {
            return;
        }
        let active_row = self.active_row;
        let outgoing_row = self.outgoing_row;
        let hidden = |row| overview_visible || (row != active_row && Some(row) != outgoing_row);
        for slot in &mut self.slots {
            if hidden(slot.row) {
                slot.animated_width.sample(now);
                slot.order_offset.sample(now);
            }
        }
        for row in 0..STRIP_COUNT {
            if !hidden(row) {
                continue;
            }
            let duration = if self.camera_touch_pan[row] {
                TOUCH_PAN_DURATION
            } else {
                transition::policy(Transition::Focus).duration
            };
            if self.camera_started[row]
                .is_some_and(|started| now.saturating_duration_since(started) >= duration)
            {
                self.camera_x[row] = self.camera_target[row];
                self.camera_started[row] = None;
                self.camera_touch_pan[row] = false;
            }
        }
    }

    fn animation_active(&self) -> bool {
        self.row_progress.is_animating()
            || self.workspace_pull.is_animating()
            || self.live_tabs.is_animating()
            || self.overview_progress.is_animating()
            || self.hints_progress.is_animating()
            || self.coach_progress.is_animating()
            || self.camera_started.iter().any(Option::is_some)
            || self
                .slots
                .iter()
                .any(|slot| slot.animated_width.is_animating() || slot.order_offset.is_animating())
    }

    /// GPUI's next-frame callback is presentation-driven. Some Wayland
    /// compositors can delay that callback despite accepting 60 Hz presents,
    /// which leaves a lightweight workspace transition visibly stepping at
    /// 20–30 Hz. A single coalesced timer wake keeps animation state advancing;
    /// the compositor still decides when the resulting frame is presented.
    fn ensure_animation_tick(&mut self, cx: &mut Context<Self>) {
        if self.animation_tick_task.is_some() {
            return;
        }
        self.animation_tick_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.animation_tick_task = None;
                if workspace.animation_active()
                    || workspace
                        .action_capture
                        .as_ref()
                        .is_some_and(ActionCapture::is_pending)
                {
                    crate::render_stats::note("workspace-notify:animation-tick");
                    cx.notify();
                }
            });
        }));
    }

    fn render_strip(
        &mut self,
        row: usize,
        viewport_w: f32,
        viewport_h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let now = Instant::now();
        self.remove_finished_closing_panels(now, window, cx);
        if row == self.active_row && self.camera_dirty[row] {
            self.resolve_camera_target(viewport_w);
        }
        if self.camera_snap_pending[row] {
            self.camera_snap_pending[row] = false;
            self.camera_started[row] = None;
            self.camera_touch_pan[row] = false;
            self.camera_from[row] = self.camera_target[row];
        }
        // Touchpad pans use a much shorter interpolation than navigation. This
        // coalesces irregular input delivery into presentation frames while
        // remaining close enough to the fingers to feel direct.
        let camera_duration = if self.camera_touch_pan[row] {
            TOUCH_PAN_DURATION
        } else {
            transition::policy(Transition::Focus).duration
        };
        match self.camera_started[row] {
            Some(started) => {
                let elapsed = started.elapsed();
                if elapsed >= camera_duration {
                    self.camera_x[row] = self.camera_target[row];
                    self.camera_started[row] = None;
                    self.camera_touch_pan[row] = false;
                } else {
                    self.camera_x[row] = smoothed_camera_position(
                        self.camera_from[row],
                        self.camera_target[row],
                        elapsed,
                        camera_duration,
                    );
                    window.request_animation_frame();
                }
            }
            None => self.camera_x[row] = self.camera_target[row],
        }

        let panel_h = (viewport_h - STRIP_PADDING_TOP - STRIP_PADDING_Y).max(1.0);
        let mut panels = Vec::with_capacity(self.row_indices(row).count());
        let mut strip_width = 0.0;
        // Sample each slot once and keep its geometry together. Four parallel
        // heap buffers used to be allocated and zipped on every pan frame.
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.row != row {
                continue;
            }
            let fraction = slot.animated_width.sample(now);
            if slot.animated_width.is_animating() {
                window.request_animation_frame();
            }
            let close_progress = slot.close_progress.sample(now);
            if slot.close_progress.is_animating() {
                window.request_animation_frame();
            }
            let width = Self::width_for_fraction(fraction, viewport_w) * close_progress;
            strip_width += width;
            let progress = slot.order_offset.sample(now);
            if slot.order_offset.is_animating() {
                window.request_animation_frame();
            }
            let distance = Self::width_for_fraction(slot.order_distance_fraction, viewport_w) + GAP;
            panels.push((index, width, progress * distance, close_progress));
        }
        let camera_x = self.camera_x[row];
        let mut strip = div()
            .absolute()
            .top(px(STRIP_PADDING_TOP))
            .left(px(-camera_x))
            .w(px(strip_width + GAP * panels.len().saturating_sub(1) as f32))
            .h(px(panel_h));

        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let mut panel_left = 0.0;
        let mut active_surface = None;
        // Capture actual painted bounds, including camera/width/order animation
        // and clipping. Keyboard focus is not a wheel hit-test target.
        let panel_hitboxes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        for (index, width, order_offset, close_progress) in panels {
            let slot = &self.slots[index];
            let focused = index == self.active;
            let content_width = px((width - if folders { 0. } else { 12. }).max(1.));
            let metadata = self.panel_metadata(slot, cx);
            let openrig = metadata.openrig;
            let pending = metadata.pending;
            let default_directory = metadata.session_id == Panel::DEFAULT_DIRECTORY_SESSION_ID;
            let machines = metadata.session_id == Panel::MACHINES_SESSION_ID;
            let needs_surface_update = metadata.surface_focused != focused
                || (focused && metadata.finished_unseen)
                || (metadata.terminal && slot.panel.needs_surface_update(focused, cx));
            if openrig
                && slot
                    .panel
                    .read_chat(cx)
                    .is_some_and(|panel| panel.openrig_width_changed(content_width, cx))
            {
                slot.panel
                    .update_chat(cx, |panel, cx| panel.set_openrig_width(content_width, cx));
            }
            // Match the selected pane, even when a menu temporarily owns keyboard focus.
            // Update only on a real change: GPUI retains views between frames and
            // treats any update while drawing as a change to what they read.
            if needs_surface_update {
                slot.panel.set_surface_focused(focused, cx);
            }
            let surface_hitboxes = panel_hitboxes.clone();
            let left = panel_left + order_offset;
            let top = FOLDER_CONTENT_INSET.min(panel_h / 2.0);
            panel_left += width + GAP;
            // Skip panels entirely outside the viewport. Every camera frame
            // moves each panel's bounds, which defeats view retention, so an
            // off-screen panel would otherwise be rebuilt, laid out and
            // prepainted on every animation frame only to be clipped. The
            // focused panel always stays mounted so keyboard focus survives.
            if !focused && !strip_panel_visible(left, width, camera_x, viewport_w) {
                continue;
            }
            let surface = div()
                .id(("panel", index))
                .absolute()
                .left(px(left + if folders { 0. } else { 6. }))
                .top(px(top))
                .opacity(close_progress)
                // Tagged so a render test can click the real panel element
                // and exercise the pointer slow-path detection.
                .debug_selector(move || format!("panel-{index}"))
                .w(content_width)
                .h(px(panel_h - top))
                .flex_none()
                // Paint focus on each pane, rather than inheriting the shared sheet.
                .bg(Theme::global().panel_background(focused))
                .when(folders, |el| el.rounded(px(CORNER_RADIUS)))
                .when(!folders, |el| {
                    el.rounded(px(CORNER_RADIUS))
                        .border_1()
                        .border_color(if focused {
                            Theme::global().PANEL_BORDER_FOCUS
                        } else {
                            Theme::global().PANEL_BORDER
                        })
                })
                .when(folders, |el| {
                    el.child(folder_surface::measure(
                        self.folder_frame.clone(),
                        folder_surface::Region::Panel { active: focused },
                    ))
                })
                .overflow_hidden()
                // Activate during capture so no child can swallow the press.
                // Composers, selectable text and buttons all stop propagation,
                // which used to leave the old panel highlighted while the
                // pressed one took keyboard focus.
                .capture_any_mouse_down(cx.listener(
                    move |this, event: &gpui::MouseDownEvent, _window, cx| {
                        if event.button == gpui::MouseButton::Left {
                            this.clicked_to_focus(index, cx);
                            this.set_active(index, cx);
                        }
                    },
                ))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        this.clicked_to_focus(index, cx);
                        this.set_active(index, cx);
                        this.focus_pending = false;
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                    }),
                )
                // Selectable transcript text intentionally consumes mouse-down
                // so it can retain keyboard focus for copy. Mouse-up still
                // bubbles, which lets an inactive panel become active without
                // stealing that focus. This is especially important after a
                // hot reload, when every restored panel contains a fresh text
                // selection model.
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, _window, cx| {
                        this.clicked_to_focus(index, cx);
                        this.set_active(index, cx);
                    }),
                )
                .child(
                    gpui::canvas(
                        move |bounds, window, _| {
                            let hitbox = window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal);
                            surface_hitboxes.borrow_mut().push((index, hitbox));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(if default_directory {
                    self.render_default_directory_picker()
                } else if machines {
                    self.render_machines(cx)
                } else if pending {
                    self.render_pending_session(index, cx)
                } else {
                    // GPUI retains every view: a panel is drawn again from the
                    // last frame while nothing it read changed, and when only
                    // the panel changed, the workspace around it is drawn from
                    // the last frame with the panel built again in its place.
                    // A `.cached` view would opt out of the latter.
                    div()
                        .size_full()
                        .child(slot.panel.view())
                        .into_any_element()
                });
            if focused {
                active_surface = Some(surface);
            } else {
                strip = strip.child(surface);
            }
        }
        // Paint the active folder last within this strip, not in a global
        // deferred layer that would incorrectly cover menus and overlays.
        strip = strip.children(active_surface);

        // Two-finger touchpad swipes (and horizontal mouse wheels) pan the
        // strip directly, like grabbing the canvas. The sticky axis lock in
        // `StripGesture` decides, per gesture, whether deltas pan the strip,
        // scroll the transcript under the pointer, or hop strips vertically.
        let total_width = self
            .row_indices(row)
            .map(|index| self.slot_width(index, viewport_w) + GAP)
            .sum::<f32>()
            + STRUT * 2.0;
        let reticle_alpha = (row == self.active_row)
            .then(|| self.gesture_reticle_alpha(cx))
            .flatten();
        if reticle_alpha.is_some() {
            // Keep painting so the reticle's hold and fade actually play out
            // after the last scroll delta.
            window.request_animation_frame();
        }
        // The reticle leans into an unbroken vertical pull, so the rubber
        // band toward the next strip is visible before it snaps.
        let reticle_pull = if self.gesture.axis == GestureAxis::Horizontal {
            (self.gesture.pull * PULL_RESISTANCE).clamp(-viewport_h / 3.0, viewport_h / 3.0)
        } else {
            0.0
        } + self.workspace_pull.value * viewport_h;
        let entity = cx.entity();
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .pl(px(GAP))
            // A capture-phase router: it sees every scroll event over the
            // strip before the transcripts do, so a gesture that committed
            // to panning can consume its vertical deltas instead of leaking
            // them into whichever panel happens to sit under the pointer.
            // The hitbox respects occlusion, so the minimap keeps its own
            // scroll behavior.
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
                    },
                    move |_, hitbox, window, _| {
                        window.on_mouse_event(
                            move |event: &gpui::ScrollWheelEvent, phase, window, cx| {
                                if phase != gpui::DispatchPhase::Capture
                                    || !hitbox.should_handle_scroll(window)
                                {
                                    return;
                                }
                                entity.update(cx, |this, cx| {
                                    // During a row transition both rows paint
                                    // a router; only the active row's routes,
                                    // and it also stops propagation for the
                                    // outgoing row's transcripts.
                                    if this.active_row != row {
                                        return;
                                    }
                                    let hovered_panel = panel_hitboxes
                                        .borrow()
                                        .iter()
                                        .rev()
                                        .find_map(|(index, hitbox)| {
                                            hitbox.should_handle_scroll(window).then_some(*index)
                                        });
                                    if this.route_strip_scroll(
                                        row,
                                        hovered_panel,
                                        event,
                                        total_width,
                                        viewport_w,
                                        window,
                                        cx,
                                    ) {
                                        cx.stop_propagation();
                                    }
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(strip)
            // The gesture reticle: while a touchpad swipe is panning this
            // strip, a ring at the camera's focal point shows exactly where
            // focus will land when the gesture settles. It fades right after
            // the fingers lift so it never lingers over content.
            .when_some(reticle_alpha, |el, alpha| {
                el.child(
                    div()
                        .debug_selector(|| "gesture-reticle".into())
                        .absolute()
                        .left(px((viewport_w - GESTURE_RETICLE_SIZE) / 2.0))
                        .top(px((viewport_h - GESTURE_RETICLE_SIZE) / 2.0 + reticle_pull))
                        .w(px(GESTURE_RETICLE_SIZE))
                        .h(px(GESTURE_RETICLE_SIZE))
                        .rounded_full()
                        .border_2()
                        .border_color(Theme::global().ACCENT)
                        .bg(gpui::rgba(0xffffff14))
                        .opacity(alpha)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(5.0))
                                .h(px(5.0))
                                .rounded_full()
                                .bg(Theme::global().ACCENT),
                        ),
                )
            })
            .into_any_element()
    }

    /// Opacity of the gesture reticle right now, or `None` once it has fully
    /// faded. Held bright while deltas keep arriving, then a short fade.
    fn gesture_reticle_alpha(&self, cx: &App) -> Option<f32> {
        self.gesture_last
            .and_then(|last| gesture_alpha(cx.background_executor().now().duration_since(last)))
    }

    /// Route one scroll event over the strip through the sticky axis lock.
    /// Returns true when the strip consumed the event and the transcripts
    /// underneath must not also scroll.
    #[allow(clippy::too_many_arguments)]
    fn route_strip_scroll(
        &mut self,
        row: usize,
        hovered_panel: Option<usize>,
        event: &gpui::ScrollWheelEvent,
        total_width: f32,
        viewport_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let delta = event.delta.pixel_delta(window.line_height());
        let mut dx = f32::from(delta.x);
        let mut dy = f32::from(delta.y);
        if event.modifiers.shift {
            // Shift redirects vertical scrolling into a horizontal pan.
            if dx == 0.0 {
                dx = dy;
            }
            dy = 0.0;
        }
        let empty_panel = hovered_panel
            .and_then(|index| self.slots.get(index))
            .filter(|slot| slot.row == row)
            .is_some_and(|slot| !slot.panel.has_scrollable_conversation(cx));
        // Use the platform clock for both gesture continuity and presentation.
        // In tests it advances explicitly, so a busy test runner cannot turn
        // consecutive touchpad events into a new gesture or expire the reticle.
        let now = cx.background_executor().now();
        if event.delta.precise() {
            // Apply continuity before every precise route, including empty
            // conversations, so small pulls separated by pauses do not add up.
            if self
                .gesture_seen
                .is_none_or(|seen| now.duration_since(seen) > GESTURE_RESET)
                || event.touch_phase == gpui::TouchPhase::Started
            {
                self.gesture.reset();
            }
            self.gesture_seen = Some(now);
        }
        let horizontally_locked = self.gesture.axis == GestureAxis::Horizontal
            && self
                .gesture_seen
                .is_some_and(|seen| now.duration_since(seen) <= GESTURE_RESET);
        // Downward workspace navigation is keyboard-only for now. Keep normal
        // transcript scrolling and horizontal panning, but do not accumulate a
        // downward pull over empty panels or during a horizontally locked pan.
        if dy < 0.0
            && (empty_panel
                || horizontally_locked
                || (self.gesture.axis == GestureAxis::Vertical && self.gesture.pull != 0.0))
        {
            self.gesture.pull = 0.0;
            dy = 0.0;
        }
        let vertical_workspace_pull = event.delta.precise()
            && self.gesture.axis == GestureAxis::Vertical
            && self.gesture.pull != 0.0;
        if (empty_panel || vertical_workspace_pull)
            && !horizontally_locked
            && dx == 0.0
            && dy != 0.0
        {
            // With no conversation beneath the pointer, vertical scrolling is
            // workspace navigation. A wheel notch moves one row immediately;
            // precise touchpad deltas accumulate to the same deliberate
            // threshold used by a vertical breakout from a horizontal pan.
            let switch = if event.delta.precise() {
                self.gesture.feed_vertical(dy, event.touch_phase)
            } else if dy < 0.0 {
                1
            } else {
                -1
            };
            if switch != 0 {
                let target = if switch > 0 {
                    (self.active_row + 1).min(STRIP_COUNT - 1)
                } else {
                    self.active_row.saturating_sub(1)
                };
                if target != self.active_row {
                    self.switch_row_animated(target, window, cx);
                }
            }
            cx.notify();
            return true;
        }
        if !event.delta.precise() {
            // Mouse wheels have no gesture continuity: horizontal clicks pan,
            // vertical ones stay with the panel under the pointer.
            if dx != 0.0 {
                self.pan_strip(row, dx, total_width, viewport_w, false, window, cx);
                self.note_strip_pan(row, -dx, false, viewport_w, window, cx);
                cx.notify();
            }
            return false;
        }
        let lifted = event.touch_phase == gpui::TouchPhase::Ended;
        match self.gesture.feed(dx, dy, event.touch_phase) {
            Routed::ToPanel => {
                if lifted {
                    // Settle a pan whose lock was released before lifting.
                    self.note_strip_pan(row, 0.0, true, viewport_w, window, cx);
                }
                false
            }
            Routed::Strip {
                dx,
                switch,
                exclusive,
            } => {
                // The reticle explains the resisted vertical pull that hops
                // strips. A purely horizontal pan needs no visualization.
                if self.gesture.axis == GestureAxis::Horizontal && dy != 0.0 {
                    self.gesture_last = Some(now);
                }
                if dx != 0.0 {
                    self.pan_strip(row, dx, total_width, viewport_w, true, window, cx);
                }
                if dx != 0.0 || lifted {
                    self.note_strip_pan(row, -dx, lifted, viewport_w, window, cx);
                }
                if switch != 0 {
                    // The vertical pull broke the sticky axis: hop to the
                    // neighbouring strip. Remaining travel is latched until
                    // the next gesture rather than skipping more strips.
                    let target = if switch > 0 {
                        (self.active_row + 1).min(STRIP_COUNT - 1)
                    } else {
                        self.active_row.saturating_sub(1)
                    };
                    if target != self.active_row {
                        self.switch_row_animated(target, window, cx);
                    }
                }
                cx.notify();
                exclusive
            }
        }
    }

    /// Pan a strip's camera directly, cancelling any in-flight animation, and
    /// keep focus attached to what the gesture has actually brought under the
    /// camera. Without the focus follow, a swipe only moved the pixels: the
    /// old off-screen panel remained active and the next keyboard action
    /// snapped back to it.
    #[allow(clippy::too_many_arguments)]
    fn pan_strip(
        &mut self,
        row: usize,
        dx: f32,
        total_width: f32,
        viewport_w: f32,
        smooth: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Natural scrolling: content follows the fingers, so the camera
        // moves opposite the delta.
        // Accumulate precise deltas onto the target, not the lagging painted
        // position. This preserves every pixel of travel while the short
        // interpolation filters event/compositor timing jitter.
        let base = if smooth {
            self.camera_target[row]
        } else {
            self.camera_x[row]
        };
        let next = pan_camera(base, -dx, total_width, viewport_w);
        if (next - base).abs() < f32::EPSILON {
            return;
        }
        self.camera_target[row] = next;
        if smooth {
            self.camera_from[row] = self.camera_x[row];
            self.camera_started[row] = Some(Instant::now());
            self.camera_touch_pan[row] = true;
        } else {
            self.camera_x[row] = next;
            self.camera_from[row] = next;
            self.camera_started[row] = None;
            self.camera_touch_pan[row] = false;
        }
        self.camera_dirty[row] = false;
        if let Some(index) = panel_at_viewport_center(
            self.slots.iter().enumerate().filter_map(|(index, slot)| {
                (slot.row == row).then_some((index, slot.width_fraction))
            }),
            next,
            viewport_w,
        ) && index != self.active
        {
            if let Some(outgoing) = self.slots.get(self.active) {
                self.previous = Some(outgoing.panel.entity_id());
            }
            self.active = index;
            self.active_row = row;
            self.focus_active(window, cx);
        }
    }

    fn render_overview(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut grid = div().relative().size_full().overflow_hidden();
        let mut columns = [0usize; STRIP_COUNT];

        for (index, slot) in self.slots.iter().enumerate() {
            let panel = self.panel_metadata(slot, cx);
            let focused = index == self.active;
            let title = panel.title.clone();
            let status = panel.status.clone();
            let preview = panel.overview_preview.clone();

            let column = columns[slot.row];
            columns[slot.row] += 1;
            let (left, top) = self.map_gesture.card_position(column, slot.row);
            grid = grid.child(
                div()
                    .id(("overview-panel", index))
                    .debug_selector(move || format!("overview-panel-{index}"))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(300.0))
                    .h(px(190.0))
                    .flex()
                    .flex_col()
                    .bg(if focused {
                        Theme::global().PANEL_BG
                    } else {
                        Theme::global().HEADER_BG
                    })
                    .rounded_xl()
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _event, window, cx| {
                            this.set_active(index, cx);
                            this.overview = false;
                            this.overview_progress.set(0.0, Instant::now());
                            this.focus_active(window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .bg(Theme::global().HEADER_BG)
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(Theme::global().TEXT)
                                    .overflow_hidden()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(Theme::global().TEXT_DIM)
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p_3()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .overflow_hidden()
                            .line_height(relative(1.4))
                            .child(preview),
                    ),
            );
        }

        // New session card.
        let (new_left, new_top) = self
            .map_gesture
            .card_position(columns[self.active_row], self.active_row);
        grid = grid.child(
            div()
                .id("overview-new")
                .absolute()
                .left(px(new_left))
                .top(px(new_top))
                .w(px(300.0))
                .h(px(190.0))
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_dashed()
                .border_color(Theme::global().PANEL_BORDER)
                .rounded_xl()
                .cursor_pointer()
                .text_color(Theme::global().TEXT_DIM)
                .hover(|el| {
                    el.border_color(Theme::global().ACCENT)
                        .text_color(Theme::global().ACCENT)
                })
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.missed("new_panel", cx);
                        this.open_new_session(cx);
                        this.overview = false;
                        this.overview_progress.set(0.0, Instant::now());
                        cx.notify();
                    }),
                )
                .child("+ new session"),
        );

        div()
            .id("session-map")
            .debug_selector(|| "session-map".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(Self::pan_session_map))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::start_map_drag))
            .on_mouse_move(cx.listener(Self::drag_session_map))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::end_map_drag))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::end_map_drag))
            .child(grid)
            .child(div().absolute().bottom_2().left_4().text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .child("Session map · two-finger scroll to pan · pinch open or select a session to return"))
            .into_any_element()
    }

    fn file_browser_root(&self, cx: &App) -> PathBuf {
        if let Some(root) = self.open_files_root(cx) {
            return root;
        }
        self.slots
            .get(self.active)
            .map(|slot| self.panel_metadata(slot, cx))
            .filter(|panel| !harness::is_remote_session(&panel.session_id))
            .and_then(|panel| panel.working_dir.as_deref().map(PathBuf::from))
            .unwrap_or_else(|| self.file_tree.borrow().fallback.clone())
    }

    fn file_entries(&self, directory: &Path, cx: &mut Context<Self>) -> std::sync::Arc<[FileRow]> {
        let mut tree = self.file_tree.borrow_mut();
        let cached = tree.rows.get(directory);
        let rows = cached.map(|(_, rows)| rows.clone()).unwrap_or_default();
        if !tree.dirty.contains(directory)
            && cached.is_some_and(|(loaded, _)| {
                self.files_pane.open || loaded.elapsed() < Duration::from_secs(2)
            })
        {
            return rows;
        }
        if let Some(reading) = &tree.reading {
            tree.needs_frame |= reading != directory;
            return rows;
        }
        let root = tree.root.clone();
        let directory = directory.to_path_buf();
        tree.dirty.remove(&directory);
        tree.reading = Some(directory.clone());
        drop(tree);
        let read = cx.background_spawn({
            let directory = directory.clone();
            async move { read_file_rows(&directory) }
        });
        cx.spawn(async move |workspace, cx| {
            let rows = read.await;
            let _ = workspace.update(cx, |workspace, cx| {
                let mut tree = workspace.file_tree.borrow_mut();
                tree.reading = None;
                let mut redraw = std::mem::take(&mut tree.needs_frame);
                if tree.root == root
                    && (root.as_ref() == Some(&directory)
                        || workspace.expanded_directories.contains(&directory))
                {
                    if tree.rows.get(&directory).is_none_or(|(_, old)| old != &rows) {
                        tree.revision += 1;
                        redraw = true;
                    }
                    tree.rows.insert(directory, (Instant::now(), rows));
                } else {
                    redraw = true;
                }
                if redraw {
                    cx.notify();
                }
            });
        })
        .detach();
        rows
    }

    fn set_file_tree_root(&self, root: &Path) {
        let mut tree = self.file_tree.borrow_mut();
        if tree.root.as_deref() != Some(root) {
            tree.rows.clear();
            tree.dirty.clear();
            tree.root = Some(root.to_path_buf());
            tree.revision += 1;
        }
    }

    fn toggle_file_directory(&mut self, path: &Path) {
        let mut tree = self.file_tree.borrow_mut();
        tree.revision += 1;
        if !self.expanded_directories.remove(path) {
            self.expanded_directories.insert(path.to_path_buf());
        } else {
            tree.rows.retain(|directory, _| !directory.starts_with(path));
            tree.dirty.retain(|directory| !directory.starts_with(path));
        }
    }

    fn render_file_entries(
        &self,
        directory: &Path,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let entries = self.file_entries(directory, cx);
        let mut rows = Vec::new();
        for FileRow { path, label, is_dir } in entries.iter().take(500) {
            let is_dir = *is_dir;
            let expanded = is_dir && self.expanded_directories.contains(path);
            let row_path = path.clone();
            let selector = label.clone();
            rows.push(
                div()
                    .id(gpui::SharedString::from(format!(
                        "file-tree-row:{}",
                        path.display()
                    )))
                    .debug_selector(move || format!("file-tree-{selector}"))
                    .pl(px(10.0 + depth as f32 * 14.0))
                    .pr_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .hover(|el| el.bg(Theme::global().HEADER_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _event, window, cx| {
                            if is_dir {
                                this.toggle_file_directory(&row_path);
                                cx.notify();
                            } else {
                                this.open_code_file(row_path.clone(), window, cx);
                            }
                        }),
                    )
                    .child(
                        div()
                            .w(px(12.0))
                            .flex_none()
                            .text_color(Theme::global().TEXT_DIM)
                            .child(if is_dir {
                                if expanded { "▾" } else { "▸" }
                            } else {
                                "·"
                            }),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label.clone()),
                    )
                    .into_any_element(),
            );
            if expanded {
                rows.extend(self.render_file_entries(path, depth + 1, cx));
            }
        }
        rows
    }

    fn scroll_sidebar_navigation_to(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let handle = &self.sidebar_navigation_scroll;
        let bounds = handle.bounds();
        let width = f32::from(bounds.size.width).max(1.0);
        let max = f32::from(handle.max_offset().x).max(0.0);
        let thumb = (width * width / (width + max)).max(28.0).min(width);
        let travel = (width - thumb).max(1.0);
        let progress =
            ((f32::from(position.x - bounds.left()) - thumb / 2.0) / travel).clamp(0.0, 1.0);
        handle.set_offset(gpui::point(px(-max * progress), px(0.0)));
        cx.stop_propagation();
        cx.notify();
    }

    fn render_sidebar_navigation_scrollbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let handle = &self.sidebar_navigation_scroll;
        let width = f32::from(handle.bounds().size.width).max(1.0);
        let max = f32::from(handle.max_offset().x).max(0.0);
        let thumb = (width * width / (width + max)).max(28.0).min(width);
        let progress = if max > 0.0 {
            (-f32::from(handle.offset().x) / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let track = div()
            .id("sidebar-navigation-scrollbar")
            .debug_selector(|| "sidebar-navigation-scrollbar".into())
            .relative()
            .w_full()
            .h(px(if folders { 16.0 } else { 6.0 }))
            .when(folders, |el| el.absolute().top(px(10.0)))
            .flex_none()
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    this.scroll_sidebar_navigation_to(event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if event.dragging() {
                    this.scroll_sidebar_navigation_to(event.position, cx);
                }
            }))
            .when(!folders, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .w_full()
                        .h(px(4.0))
                        .rounded_full()
                        .bg(Theme::global().PANEL_BORDER),
                )
            })
            .when(!folders, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .left(relative((width - thumb) * progress / width))
                        .w(relative(thumb / width))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(Theme::global().TEXT_FAINT),
                )
            });
        div()
            .relative()
            .w_full()
            .h(px(if folders { 10.0 } else { 6.0 }))
            .flex_none()
            .child(track)
            .into_any_element()
    }

    fn render_files_sidebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self
            .slots
            .get(self.active)
            .is_some_and(|slot| harness::is_remote_session(&self.panel_metadata(slot, cx).session_id))
        {
            return div().debug_selector(|| "remote-file-browser-notice".into()).p_3().text_size(px(11.0)).text_color(Theme::global().TEXT_DIM)
                .child("This panel runs on a remote machine. Ask Jcode to inspect its files in chat. The local file browser is available on local panels.")
                .into_any_element();
        }
        let root = self.file_browser_root(cx);
        self.set_file_tree_root(&root);
        div()
            .id("sidebar-file-list")
            .debug_selector(|| "sidebar-file-list".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.sidebar_scroll)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .font_family(Theme::global().FONT_MONO)
                    .text_size(px(10.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(root.display().to_string()),
            )
            .children(self.render_file_entries(&root, 0, cx))
            .into_any_element()
    }

    fn render_sidebar_scrollbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let sessions = self.sidebar_view == SidebarView::Sessions;
        let outset = if self.layout_mode == crate::config::LayoutMode::FolderTabs {
            SIDEBAR_SCROLLBAR_OUTSET
        } else {
            0.0
        };
        div()
            .id("sidebar-scroll-gutter")
            .debug_selector(|| "sidebar-scroll-gutter".into())
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-outset))
            .w(px(crate::scrollbar::GUTTER))
            // The gutter is a sibling overlay, not part of the scrollable
            // content. Handle its wheel input just like the folder-tab strip,
            // and consume it so the list underneath cannot scroll twice.
            .on_scroll_wheel(cx.listener(
                move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let dy = event.delta.pixel_delta(window.line_height()).y;
                    if sessions {
                        this.sidebar_sessions_list.scroll_by(-dy);
                    } else {
                        let handle = &this.sidebar_scroll;
                        let y = (handle.offset().y + dy).clamp(-handle.max_offset().y, px(0.0));
                        handle.set_offset(gpui::point(px(0.0), y));
                    }
                    cx.stop_propagation();
                    cx.notify();
                },
            ))
            .child(if sessions {
                crate::scrollbar::vertical_list(&self.sidebar_sessions_list, "sidebar-scrollbar")
            } else {
                crate::scrollbar::vertical_with_track(&self.sidebar_scroll, "sidebar-scrollbar")
            })
            .into_any_element()
    }

    /// Replace the set of sessions the daemon is running. Retains the existing
    /// mark of a session that is still running so its animation continues.
    fn set_daemon_running(&mut self, running: HashSet<String>, cx: &mut Context<Self>) {
        if running.len() == self.daemon_running.len()
            && running
                .iter()
                .all(|id| self.daemon_running.contains_key(id))
        {
            return;
        }
        self.daemon_running.retain(|id, _| running.contains(id));
        for id in running {
            self.daemon_running
                .entry(id)
                .or_insert_with(|| Panel::daemon_sidebar_mark(cx));
        }
        cx.notify();
    }

    /// Sidebar activity for a session without an open panel.
    fn daemon_running_mark(&self, session_id: &str) -> Option<gpui::AnyView> {
        self.daemon_running.get(session_id).cloned()
    }

    fn render_sidebar(&mut self, fullscreen: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Release checks and the packaged automatic-update loop start here, on
        // the first frame of the window, not from the avatar's update badge: a
        // fresh packaged launch has no release state yet, so there is nothing
        // for the badge to show until this runs. Idempotent, so every later
        // render is free.
        updates::ensure_release_check();
        let sidebar_panels = self
            .sidebar_panel_metadata
            .as_ref()
            .expect("sidebar metadata prepared before render")
            .read(cx);
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| sidebar_panels[&slot.panel.entity_id()].session_id.clone());
        let open_marks = self
            .slots
            .iter()
            .filter(|slot| !slot.closing)
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.sidebar_activity())
            })
            .collect::<HashMap<_, _>>();
        let open_todos = self
            .slots
            .iter()
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.latest_todo_progress())
            })
            .collect::<HashMap<_, _>>();
        let working = open_marks
            .iter()
            .filter(|(_, mark)| mark.is_some())
            .map(|(id, _)| id.clone())
            .chain(
                self.daemon_running
                    .keys()
                    .filter(|id| !open_marks.contains_key(*id))
                    .cloned(),
            )
            .collect::<HashSet<_>>();
        self.sidebar_title_pulses.retain(|id| working.contains(id));
        let open_titles = self
            .slots
            .iter()
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.title.to_string())
            })
            .collect::<HashMap<_, _>>();
        // A session which already has a panel is navigation. Everything else
        // is an invitation to open another panel, so keep those two actions in
        // visibly separate sections. Preserve the TUI saved/recency ordering
        // within each section.
        // Live panels are authoritative, including sessions still waiting for
        // creation/catalog acknowledgement. Never wait for a server refresh to
        // make a newly opened neighbor reachable in the sidebar.
        let mut catalog = self.sessions.clone();
        let mut known = catalog
            .iter()
            .map(|session| session.session_id.clone())
            .collect::<HashSet<_>>();
        // Keep legacy first-record lookup semantics even for duplicate catalog ids.
        let duplicate_catalog_ids = known.len() != catalog.len();
        for slot in self.slots.iter().filter(|slot| !slot.closing) {
            let panel = &sidebar_panels[&slot.panel.entity_id()];
            if (panel.supports_voice() || harness::is_remote_session(&panel.session_id))
                && panel.listable
                && known.insert(panel.session_id.clone())
            {
                catalog.push(jcode_sdk::SessionInfo {
                    user_connected: false,
                    session_id: panel.session_id.clone(),
                    working_dir: panel.working_dir.clone(),
                    title: Some(panel.title.to_string()),
                    status: "active".into(),
                    transcript_bytes: None,
                    saved: false,
                    updated_at_ms: None,
                    last_active_at_ms: None,
                    archived: false,
                    archived_at_ms: None,
                    save_label: None,
                    parent_session_id: None,
                    agent_label: None,
                    swarm_status: None,
                    edit_stats: None,
                    context: None,
                });
            }
        }
        // Only this window's explicitly closed sessions belong in Past. Keep
        // the full runtime catalog elsewhere for search and session recovery.
        let mut kept = catalog
            .iter()
            .filter(|session| {
                open_marks.contains_key(&session.session_id)
                    || session.user_connected
                    || self.sidebar_closed_sessions.contains(&session.session_id)
            })
            .map(|session| session.session_id.clone())
            .collect::<HashSet<_>>();
        // The TUI shows a coordinator's workers while nobody has that
        // coordinator open here. A live swarm is live work, not a window's
        // property, so list it wherever it is driven from.
        // A worker's own record can outrun a terminal swarm snapshot (a daemon
        // reload rewrites nothing), so liveness reads both.
        let live_swarms =
            sidebar_swarm::live_swarm_roots(&catalog, &self.worker_meta, swarm_now_ms());
        kept.extend(live_swarms.iter().cloned());
        // Headless swarm workers are never opened or closed in this window,
        // but they belong under a coordinator that is listed here.
        sidebar_swarm::keep_workers_of(&catalog, &mut kept);
        let preferences = self.session_catalog;
        catalog.retain(|session| {
            preferences.origin.matches(session)
                && (kept.contains(&session.session_id)
                    || (session.is_agent_started()
                        && (preferences.origin == sidebar_session_catalog::OriginFilter::Agents
                            || working.contains(&session.session_id))))
        });
        let original_catalog = duplicate_catalog_ids.then(|| catalog.clone());
        // Sort the owned render catalog in place, avoiding a second session clone.
        sort_sidebar_sessions(&mut catalog, preferences.sort);
        let ordered_sessions = catalog;
        let swarm = sidebar_swarm::groups(&ordered_sessions);
        let reply_times: HashMap<_, _> =
            if preferences.sort == sidebar_session_catalog::Sort::AgentActivity {
                ordered_sessions
                    .iter()
                    .filter_map(|session| {
                        Some((
                            session.session_id.clone(),
                            session.last_agent_activity_at_ms()?,
                        ))
                    })
                    .collect()
            } else {
                HashMap::new()
            };
        // Swarms and worktrees are one concept here: an agent is a thread. It
        // nests under its coordinator while it shares that checkout, and
        // surfaces under its own branch when it works in a separate worktree.
        let mut agents: HashMap<String, (usize, usize)> = HashMap::new();
        let mut nested_workers: HashMap<String, Vec<(usize, crate::swarm_view::Worker)>> =
            HashMap::new();
        let swarm_now = swarm_now_ms();
        let mut nested = HashSet::new();
        for (root, children) in &swarm.children {
            let root_checkout = original_catalog
                .as_ref()
                .unwrap_or(&ordered_sessions)
                .iter()
                .find(|session| &session.session_id == root)
                .and_then(|session| self.sidebar_projects.checkout(session))
                .map(|checkout| checkout.path);
            for child in children {
                let own = self
                    .sidebar_projects
                    .checkout(&child.session)
                    .map(|checkout| checkout.path);
                let worker = crate::swarm_view::Worker::from_session(
                    &child.session,
                    self.worker_meta.get(&child.session.session_id),
                    swarm_now,
                );
                let separate_checkout = own.is_some() && own != root_checkout;
                if separate_checkout && !worker.state.is_live() {
                    continue;
                }
                // Live worktree agents remain visible under their coordinator
                // and in their own checkout.
                if !separate_checkout {
                    nested.insert(child.session.session_id.clone());
                }
                let running = worker.state == crate::swarm_view::WorkerState::Running;
                nested_workers
                    .entry(root.clone())
                    .or_default()
                    .push((child.depth, worker));
                let entry = agents.entry(root.clone()).or_default();
                entry.0 += 1;
                if running {
                    entry.1 += 1;
                }
            }
        }
        // Long-running coordinators own dozens of workers. Show the live ones
        // and the latest few finished, fold the rest into "+N finished".
        let mut folded_workers: HashMap<String, usize> = HashMap::new();
        for (root, workers) in nested_workers.iter_mut() {
            workers.sort_by(|(_, a), (_, b)| {
                crate::swarm_view::spawn_order(&a.session_id)
                    .cmp(&crate::swarm_view::spawn_order(&b.session_id))
            });
            let (shown, hidden) = sidebar_swarm::visible(workers);
            if !self
                .expanded_lines
                .contains(&sidebar_swarm::finished_key(root))
            {
                *workers = shown;
            }
            if hidden > 0 {
                folded_workers.insert(root.clone(), hidden);
            }
            if preferences.sort == sidebar_session_catalog::Sort::AgentActivity {
                workers.sort_by_key(|(_, worker)| {
                    std::cmp::Reverse(reply_times.get(&worker.session_id))
                });
            }
        }
        // Fading local panels stay mounted, but must not reappear as history
        // or stale globally connected rows before their close completes.
        let closing_sessions = self
            .slots
            .iter()
            .filter(|slot| slot.closing)
            .map(|slot| sidebar_panels[&slot.panel.entity_id()].session_id.clone())
            .collect::<HashSet<_>>();
        let (mut open_sessions, other_sessions): (Vec<_>, Vec<_>) = ordered_sessions
            .into_iter()
            .filter(|session| {
                !nested.contains(&session.session_id)
                    && !closing_sessions.contains(&session.session_id)
            })
            .partition(|session| {
                open_marks.contains_key(&session.session_id)
                    || session.user_connected
                    // A coordinator with live workers is live work even when
                    // this window has no panel for it: putting it in history
                    // would fold it into a collapsed project.
                    || live_swarms.contains(&session.session_id)
            });
        let mut panel_positions = self.slots.iter().enumerate().collect::<Vec<_>>();
        panel_positions.sort_by_key(|(slot_index, slot)| (slot.row, *slot_index));
        let panel_positions = panel_positions
            .into_iter()
            .enumerate()
            .map(|(position, (_, slot))| {
                (
                    sidebar_panels[&slot.panel.entity_id()].session_id.clone(),
                    position,
                )
            })
            .collect::<HashMap<_, _>>();
        if preferences.sort == sidebar_session_catalog::Sort::Recent {
            open_sessions.sort_by_key(|session| {
                panel_positions
                    .get(&session.session_id)
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        // External user clients are live too, but are not locally open panels.
        // Keep their rows without changing navigation or selection semantics.
        // Group by project (Git repository, else spawn directory). Projects
        // appear in the order of their first open panel, then by history
        // recency. Inside a project, threads group by checkout (the main
        // working tree first, then linked worktrees), and open panels precede
        // history within each checkout.
        //
        // User-created collections come first, in their own order, and take
        // their sessions out of the directory groups below.
        let mut groups: Vec<(sidebar_projects::Project, usize, usize)> = Vec::new();
        let mut group_indices: HashMap<String, usize> = HashMap::new();
        let mut members: Vec<Vec<SidebarRow>> = Vec::new();
        let mut project_checkouts: Vec<Vec<sidebar_projects::Checkout>> = Vec::new();
        let collection_order = self
            .collections
            .store
            .collections
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let rank_collection = |session: &jcode_sdk::SessionInfo| {
            self.collections
                .collection_of(&session.session_id)
                .and_then(|c| collection_order.iter().position(|id| *id == c.id))
                .unwrap_or(usize::MAX)
        };
        let mut grouped = open_sessions
            .into_iter()
            .map(|session| (true, session))
            .chain(other_sessions.into_iter().map(|session| (false, session)))
            .collect::<Vec<_>>();
        // Stable: open-before-history and recency survive inside each group.
        grouped.sort_by_key(|(_, session)| rank_collection(session));
        for (live, session) in grouped {
            // open_marks contains exactly the non-closing local panel ids.
            let open = open_marks.contains_key(&session.session_id);
            let collection = self
                .collections
                .collection_of(&session.session_id)
                .map(|c| (c.id.clone(), c.name.clone()));
            let (project, subdirectory) = match collection {
                // Inside a collection, the session's folder is a detail line.
                Some((id, name)) => (
                    sidebar_projects::Project {
                        key: sidebar_collections::key(&id),
                        label: name,
                        location: None,
                        git: false,
                    },
                    sidebar_session_directory(&session),
                ),
                None => self.sidebar_projects.project(&session),
            };
            // Collections mix folders, so they never split into checkouts.
            let checkout = if sidebar_collections::id_from_key(&project.key).is_some() {
                None
            } else {
                self.sidebar_projects.checkout(&session)
            };
            let index = match group_indices.get(&project.key).copied() {
                Some(index) => index,
                None => {
                    group_indices.insert(project.key.clone(), groups.len());
                    groups.push((project, 0, 0));
                    members.push(Vec::new());
                    project_checkouts.push(Vec::new());
                    groups.len() - 1
                }
            };
            if let Some(checkout) = &checkout
                && !project_checkouts[index]
                    .iter()
                    .any(|c| c.path == checkout.path)
            {
                project_checkouts[index].push(checkout.clone());
            }
            // Inside a linked worktree the path relative to the main checkout
            // is noise. The checkout header already names the branch.
            let subdirectory = match &checkout {
                Some(checkout) if checkout.linked => session
                    .working_dir
                    .as_deref()
                    .and_then(|dir| Path::new(dir).strip_prefix(&checkout.path).ok())
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.to_string_lossy().into_owned()),
                _ => subdirectory,
            };
            groups[index].1 += usize::from(live);
            groups[index].2 += 1;
            let agents = agents.get(&session.session_id).copied().unwrap_or_default();
            members[index].push(SidebarRow {
                open,
                live,
                session,
                project: index,
                subdirectory,
                header: false,
                collapsed: false,
                divider: false,
                checkout,
                checkout_header: false,
                section: None,
                agents,
            });
        }
        // The main working tree leads, then worktrees in order of first use.
        for checkouts in &mut project_checkouts {
            checkouts.sort_by_key(|checkout| checkout.linked);
        }
        for (index, rows) in members.iter_mut().enumerate() {
            let rank = |row: &SidebarRow| {
                row.checkout
                    .as_ref()
                    .and_then(|c| {
                        project_checkouts[index]
                            .iter()
                            .position(|p| p.path == c.path)
                    })
                    .unwrap_or(usize::MAX)
            };
            rows.sort_by_key(rank);
        }
        // Branch rows appear once a project spans several checkouts. A single
        // checkout shows its branch on the project header instead.
        let checkout_rows = project_checkouts
            .iter()
            .map(|checkouts| checkouts.len() > 1 || checkouts.iter().any(|c| c.linked))
            .collect::<Vec<_>>();
        let project_branches = project_checkouts
            .iter()
            .zip(&checkout_rows)
            .map(|(checkouts, rows)| {
                (!rows)
                    .then(|| checkouts.first().map(|c| c.branch.clone()))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let active_project = active_id.as_deref().and_then(|id| {
            members
                .iter()
                .position(|rows| rows.iter().any(|row| row.session.session_id == id))
        });
        self.sidebar_projects.sync_focus(
            active_id.as_deref(),
            active_project.map(|index| groups[index].0.key.as_str()),
        );
        let headers = groups.len() > 1
            || groups
                .first()
                .is_some_and(|(project, _, _)| project.key != sidebar_projects::OTHER);
        let any_live = groups.iter().any(|(_, live, _)| *live > 0);
        let collapsed_projects = groups
            .iter()
            .map(|(project, live, _)| {
                // A collection the user built stays open unless they fold it.
                let default = sidebar_collections::id_from_key(&project.key).is_none()
                    && any_live
                    && *live == 0;
                headers && self.sidebar_projects.collapsed(&project.key, default)
            })
            .collect::<Vec<_>>();
        let mut ordered_sessions = Vec::new();
        for (index, rows) in members.into_iter().enumerate() {
            let collapsed = collapsed_projects[index];
            let mut previous: Option<(bool, bool)> = None;
            let mut previous_checkout: Option<Option<String>> = None;
            for (position, mut row) in rows.into_iter().enumerate() {
                if collapsed && position > 0 {
                    // One representative keeps the header a real list row.
                    break;
                }
                row.header = headers && position == 0;
                row.collapsed = collapsed;
                let checkout = row.checkout.as_ref().map(|c| c.path.clone());
                if previous_checkout.as_ref() != Some(&checkout) {
                    row.checkout_header = checkout_rows[index] && !collapsed;
                    previous = None;
                }
                previous_checkout = Some(checkout);
                // Live includes externally connected user clients, while
                // row.open continues to mean navigation to a local panel.
                if !collapsed && previous.is_none_or(|(live, _)| live != row.live) {
                    row.section = Some(if row.live { "Live" } else { "Past" });
                }
                row.divider = !row.live
                    && previous.is_some_and(|(live, saved)| !live && saved && !row.session.saved);
                previous = Some((row.live, row.session.saved));
                ordered_sessions.push(row);
            }
        }
        let selection_order = ordered_sessions
            .iter()
            .filter(|row| row.open && !row.collapsed)
            .map(|row| row.session.session_id.clone())
            .collect::<Vec<_>>();
        self.sidebar_selection.retain(&selection_order);
        // Keep only visible rows (plus a small overscan) in GPUI's layout and
        // text-shaping work. Hover invalidates this view, so eagerly building
        // all history rows here makes pointer latency scale with history size.
        let layout = ordered_sessions
            .iter()
            .map(|row| {
                let session = &row.session;
                let selected = active_id.as_deref() == Some(session.session_id.as_str());
                SidebarSessionLayout {
                    session_id: session.session_id.clone(),
                    open: row.open,
                    project: row.project,
                    header: row.header,
                    checkout_header: row.checkout_header,
                    divider: row.divider,
                    section: row.section,
                    collapsed: row.collapsed,
                    selected,
                    saved: session.saved,
                    details: (row.subdirectory.is_some()
                        || sidebar_session_created_ms(&session.session_id).is_some()
                        || session.transcript_bytes.is_some_and(|bytes| bytes > 0)),
                    menu: self.collections.menu.as_deref() == Some(session.session_id.as_str()),
                    workers: (!row.collapsed)
                        .then(|| nested_workers.get(&session.session_id))
                        .flatten()
                        .map(|workers| {
                            sidebar_swarm::layout_key(
                                workers,
                                folded_workers
                                    .get(&session.session_id)
                                    .copied()
                                    .unwrap_or(0),
                            )
                        })
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();
        let newly_opened_active = layout.iter().position(|row| {
            row.open
                && row.selected
                && !self
                    .sidebar_session_layout
                    .iter()
                    .any(|old| old.open && old.session_id == row.session_id)
        });
        sync_sidebar_session_layout(
            &self.sidebar_sessions_list,
            &mut self.sidebar_session_layout,
            layout,
        );
        if let Some(index) = newly_opened_active {
            self.sidebar_sessions_list.scroll_to_reveal_item(index);
        }
        // Render read cached Git state only. Probe what it lacked off-thread.
        self.sidebar_projects.probe_in_background(cx);
        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let folder_frame = self.folder_frame.clone();
        let workspace = cx.entity();
        let mut list = div()
            .id("sidebar-session-list")
            .debug_selector(|| "sidebar-session-list".into())
            .flex_1()
            .min_h_0()
            .overflow_hidden();
        list = list.flex().flex_col();
        list = list.child(self.render_sidebar_quick_actions(cx));
        if let Some(applets) = self.sidebar_applets.take() {
            list = list.child(applets);
        }
        if let Some(form) = self.render_worktree_form(cx) {
            list = list.child(form);
        }
        if let Some(form) = self.render_collection_form(cx) {
            list = list.child(form);
        }
        // Collections with no chats yet have no row to carry a header.
        let empty_collections = self
            .collections
            .store
            .collections
            .iter()
            .filter(|c| {
                !groups
                    .iter()
                    .any(|(p, _, _)| p.key == sidebar_collections::key(&c.id))
            })
            .cloned()
            .collect::<Vec<_>>();
        if let Some(empty) = self.render_empty_collections(&empty_collections, cx) {
            list = list.child(empty);
        }
        if !self.sidebar_selection.ids.is_empty() {
            list = list.child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.0))
                    .child(format!("{} selected", self.sidebar_selection.ids.len()))
                    .child(
                        div()
                            .id("sidebar-close-selected")
                            .debug_selector(|| "sidebar-close-selected".into())
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(Theme::global().ACCENT_DIM)
                            .cursor_pointer()
                            .child("Close selected")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    let ids = this
                                        .sidebar_session_layout
                                        .iter()
                                        .filter(|s| this.sidebar_selection.contains(&s.session_id))
                                        .map(|s| s.session_id.clone())
                                        .collect();
                                    this.close_sidebar_sessions(ids, window, cx);
                                }),
                            ),
                    ),
            );
        }
        if !ordered_sessions.is_empty() {
            list = list.child(div().relative().w_full().flex_1().min_h_0().child(
                gpui::list(
                    self.sidebar_sessions_list.clone(),
                    move |sidebar_index, _window, cx| {
                        let row = &ordered_sessions[sidebar_index];
                        let is_open = row.open;
                        let live = row.live;
                        let session = &row.session;
                        workspace.update(cx, |this, cx| {
                            let mut list = div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .relative()
                                .when(sidebar_index == 0, |el| el.pt_2())
                                .when(sidebar_index + 1 == ordered_sessions.len(), |el| el.pb_2());
                            if row.header {
                                let (project, _, count) = &groups[row.project];
                                let header = if sidebar_collections::id_from_key(&project.key).is_some() {
                                    this.render_collection_header(row.project, project, *count, collapsed_projects[row.project], cx)
                                } else {
                                    this.render_project_header(row.project, project, *count, collapsed_projects[row.project], project_branches[row.project].clone(), cx)
                                };
                                list = list
                                    .when(sidebar_index > 0, |el| el.pt_3())
                                    .child(header);
                            }
                            if row.checkout_header {
                                list = list.child(this.render_checkout_header(sidebar_index, row.checkout.as_ref(), cx));
                            }
                            if row.collapsed {
                                return list.into_any_element();
                            }
                            if let Some(section) = row.section {
                                list = list.child(
                                    div()
                                        .debug_selector(move || format!("sidebar-section-{sidebar_index}"))
                                        .ml(px(if checkout_rows[row.project] { 30.0 } else { 20.0 }))
                                        .mt(px(2.0))
                                        .mb(px(2.0))
                                        .text_size(px(9.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Theme::global().TEXT_FAINT)
                                        .child(section),
                                );
                            }
                            if row.divider {
                                list = list.child(
                                    div()
                                        .id("sidebar-session-divider")
                                        .debug_selector(|| "sidebar-session-divider".into())
                                        .mx_4()
                                        .my_2()
                                        .border_t_1()
                                        .border_color(Theme::global().PANEL_BORDER),
                                );
                            }
                            let selected =
                                active_id.as_deref() == Some(session.session_id.as_str());
                            let (icon, title) = sidebar_session_title_with_open_title(
                                session,
                                open_titles.get(&session.session_id).map(String::as_str),
                            );
                            #[cfg(test)]
                            tests::SIDEBAR_TITLE_RENDERS.with(|count| count.set(count.get() + 1));
                            let directory = row.subdirectory.clone();
                            // Todo progress leads the details line: live from an
                            // open panel, otherwise from the persisted snapshot.
                            let todo_progress = open_todos
                                .get(&session.session_id)
                                .copied()
                                .flatten()
                                .or_else(|| this.sidebar_todo_progress.get(&session.session_id).copied());
                            let todo_session = session.clone();
                            let meta = sidebar_session_meta(session);
                            let details = match (directory, meta) {
                                (Some(directory), Some(meta)) => {
                                    Some(format!("{directory} · {meta}"))
                                }
                                (Some(directory), None) => Some(directory),
                                (None, Some(meta)) => Some(meta),
                                (None, None) => None,
                            };
                            // An open panel owns its richer live mark. Otherwise
                            // show the daemon's view, so sessions running in the
                            // background (other clients, swarm workers, closed
                            // panels) still spin in the sidebar.
                            let activity = match open_marks.get(&session.session_id) {
                                Some(mark) => mark.clone(),
                                None => this.daemon_running_mark(&session.session_id),
                            };

                            let group_selected = is_open && this.sidebar_selection.contains(&session.session_id);
                            let (_, agent_working) = row.agents;
                            let running_workers = nested_workers.get(&session.session_id).into_iter().flatten()
                                .filter(|(_, worker)| worker.state == crate::swarm_view::WorkerState::Running)
                                .map(|(_, worker)| worker.clone()).collect::<Vec<_>>();
                            let under_checkout = checkout_rows[row.project];
                            let close_id = session.session_id.clone();
                            let pin_id = session.session_id.clone();
                            let pinned = session.saved;
                            let agent_started = session.is_agent_started();
                            let release_id = session.session_id.clone();
                            let release_out_id = session.session_id.clone();
                            let context_id = session.session_id.clone();
                            let menu_open = this.collections.menu.as_deref() == Some(session.session_id.as_str());
                            let menu_session = session.session_id.clone();
                            let selection_order = selection_order.clone();
                            let hovered = this.sidebar_hovered_session.as_deref() == Some(session.session_id.as_str());
                            let working = activity.is_some();
                            let hover_id = session.session_id.clone();
                            let title_pulse = working.then(|| {
                                this.sidebar_title_pulses.get(
                                    &session.session_id,
                                    crate::pulse_text::Kind::PulseText {
                                        text: title.clone().into(),
                                        period: crate::pulse_text::TOOL_PULSE_PERIOD,
                                        curve: sidebar_title_pulse,
                                    },
                                    cx,
                                )
                            });
                            let session = session.clone();
                            let keyboard_session = session.clone();
                            list = list.child(
                                crate::keyboard::button(div().id(("sidebar-session", sidebar_index)))
                                    .on_click(cx.listener(move |this, event, window, cx| {
                                        if matches!(event, gpui::ClickEvent::Keyboard(_)) {
                                            cx.stop_propagation();
                                            this.activate_session(keyboard_session.clone(), window, cx);
                                        }
                                    }))
                                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                                        let current = this.sidebar_hovered_session.as_deref() == Some(hover_id.as_str());
                                        if *hovering && !current {
                                            this.sidebar_hovered_session = Some(hover_id.clone());
                                            cx.notify();
                                        } else if !*hovering && current {
                                            this.sidebar_hovered_session = None;
                                            cx.notify();
                                        }
                                    }))
                                    .group("sidebar-session-row")
                                    .debug_selector(move || {
                                        format!("sidebar-session-{sidebar_index}")
                                    })
                                    .ml_2()
                                    .when(under_checkout, |el| el.ml(px(18.0)))
                                    .mr_2()
                                    .mb(px(2.0))
                                    .relative()
                                    .px_2()
                                    .py(px(2.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .when(selected && folders, |el| el.child(folder_surface::measure(
                                        folder_frame.clone(), folder_surface::Region::SelectedTab,
                                    )))
                                    .when(!folders, |el| el.mr_2().rounded_lg().bg(if selected {
                                        Theme::global().PANEL_BG
                                    } else {
                                        Theme::global().HEADER_BG
                                    }))
                                    .border_l_1()
                                    .border_t_1()
                                    .border_b_1()
                                    .border_color(gpui::rgba(0x00000000))
                                    .when(group_selected, |el| el.bg(Theme::global().ACCENT_DIM).border_color(Theme::global().ACCENT))
                                    .pr(px(crate::scrollbar::GUTTER + 8.0))
                                    .when(!selected, |el| el.hover(move |el| el.bg(Theme::global().PANEL_BG)))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, _cx| {
                                            // Release will explicitly focus the session's composer.
                                            // Otherwise the focusable workspace ancestor handles
                                            // this same press afterward and steals focus back.
                                            window.prevent_default();
                                            this.begin_sidebar_gesture(session.clone(), selection_order.clone(), is_open, event);
                                        }),
                                    )
                                    .on_mouse_up(gpui::MouseButton::Left, cx.listener(move |this, event, window, cx| {
                                        this.finish_sidebar_gesture(&release_id, true, event, window, cx);
                                    }))
                                    .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(move |this, event, window, cx| {
                                        this.finish_sidebar_gesture(&release_out_id, false, event, window, cx);
                                    }))
                                    // Right-click opens the collection menu.
                                    .on_mouse_down(gpui::MouseButton::Right, cx.listener(move |this, _, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.sidebar_gesture = None;
                                        this.collections.menu = Some(context_id.clone());
                                        cx.notify();
                                    }))
                                    .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                        this.sidebar_gesture = None;
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .h(px(18.0))
                                            .child(
                                                div()
                                                    .w(px(16.0))
                                                    .flex_none()
                                                    .text_size(px(12.0))
                                                    .child(icon),
                                            )
                                            .child({
                                                let container = div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .max_h(px(18.0))
                                                    .truncate()
                                                    .debug_selector(move || {
                                                        format!(
                                                            "sidebar-session-title-{sidebar_index}"
                                                        )
                                                    })
                                                    .text_size(px(12.0))
                                                    .line_height(relative(1.5))
                                                    // Past sessions stay listed but read dimmer than live ones.
                                                    .when(!live, |el| el.text_color(Theme::global().TEXT_DIM));
                                                // A working session's title breathes in step
                                                // with running tool rows. The pulse is its own
                                                // view, so a tick redraws only the title instead
                                                // of the whole workspace.
                                                match title_pulse {
                                                    Some(pulse) => container.child(pulse),
                                                    None => container.child(title),
                                                }
                                            })
                                            .children(agent_started.then(|| sidebar_session_catalog::agent_badge()))
                                            .when(agent_working > 0, |row| row.child(
                                                div()
                                                    .id(("sidebar-agents", sidebar_index))
                                                    .debug_selector(move || format!("sidebar-agents-{sidebar_index}"))
                                                    .flex_none().h(px(16.0)).px(px(6.0))
                                                    .flex().items_center().gap(px(3.0)).rounded_full()
                                                    .bg(Theme::global().ACCENT_DIM)
                                                    .text_size(px(9.0)).text_color(Theme::global().TEXT_DIM)
                                                    .cursor_pointer()
                                                    .tooltip(move |_, cx| cx.new(|_| sidebar_header::WorkersTooltip(running_workers.clone())).into())
                                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                        window.prevent_default();
                                                        cx.stop_propagation();
                                                        this.sidebar_gesture = None;
                                                        this.sidebar_sessions_list.scroll_to(gpui::ListOffset { item_ix: sidebar_index, offset_in_item: px(0.0) });
                                                        cx.notify();
                                                    }))
                                                    .child(gpui::svg().data(include_bytes!("../../../assets/icons/swarm.svg").as_slice()).size(px(9.0)).text_color(Theme::global().TEXT_DIM))
                                                    .child(div().debug_selector(move || format!("sidebar-agents-count-{sidebar_index}-{agent_working}")).child(agent_working.to_string())),
                                            ))
                                            // Idle rows keep the pin visible when pinned. Hovering
                                            // swaps any spinner for the pin and close actions, so the
                                            // title can use the full width the rest of the time.
                                            .when(hovered || (pinned && !working), |row| row
                                                // Pinning is /save: pinned sessions sort to the top.
                                                .child(
                                                    div()
                                                        .id(("sidebar-pin", sidebar_index))
                                                        .debug_selector(move || format!("sidebar-pin-{sidebar_index}"))
                                                        .flex_none()
                                                        .size(px(18.0))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_full()
                                                        .cursor_pointer()
                                                        .hover(|el| el.bg(Theme::global().TOOL_BG))
                                                        .tooltip(move |_, cx| {
                                                            cx.new(|_| remotes::HeaderTooltip(
                                                                if pinned { "Unpin session" } else { "Pin session" }.into(),
                                                            ))
                                                            .into()
                                                        })
                                                        .child(
                                                            gpui::svg()
                                                                .data(include_bytes!("../../../assets/icons/pin.svg").as_slice())
                                                                .size(px(11.0))
                                                                .text_color(if pinned {
                                                                    Theme::global().ACCENT
                                                                } else {
                                                                    Theme::global().TEXT_DIM
                                                                }),
                                                        )
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.sidebar_gesture = None;
                                                            this.set_session_pinned(&pin_id, !pinned, cx);
                                                        })),
                                                )
                                                // Last child: the hover close sits at the row's far right.
                                                .when(is_open && hovered, |row| row.child(
                                                    div().id(("sidebar-close", sidebar_index))
                                                        .debug_selector(move || format!("sidebar-close-{sidebar_index}"))
                                                        .flex_none().size(px(18.0)).flex().items_center().justify_center().cursor_pointer().child("×")
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.sidebar_gesture = None;
                                                            this.close_sidebar_sessions(vec![close_id.clone()], window, cx);
                                                        }))
                                                ))
                                            )
                                            // Last child: the spinner sits at the row's far right.
                                            .when_some(activity.filter(|_| !hovered), |row, spinner| {
                                                row.child(
                                                    div()
                                                        .debug_selector(move || {
                                                            format!("sidebar-session-spinner-{sidebar_index}")
                                                        })
                                                        .flex_none()
                                                        .child(spinner),
                                                )
                                            })
                                    )
                                    .when(details.is_some() || todo_progress.is_some(), |row| {
                                        row.child(
                                            div()
                                                .when(todo_progress.is_none(), |line| line.pl(px(20.0)))
                                                .flex()
                                                .items_center()
                                                .gap(px(3.0))
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_size(px(9.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .when_some(todo_progress, |line, (done, total)| line.child(
                                                    crate::keyboard::press_button(
                                                        div().id(("sidebar-todos", sidebar_index)),
                                                        {
                                                            let todo_session = todo_session.clone();
                                                            let entity = cx.weak_entity();
                                                            move |window, cx| {
                                                                window.prevent_default();
                                                                cx.stop_propagation();
                                                                let _ = entity.update(cx, |this, cx| {
                                                                    this.sidebar_gesture = None;
                                                                    this.show_orchestration(Some(todo_session.clone()), window, cx);
                                                                });
                                                            }
                                                        },
                                                    )
                                                        .min_w_0().flex().items_center().gap(px(2.0))
                                                        .relative()
                                                        .debug_selector(move || format!("sidebar-todos-{sidebar_index}"))
                                                        .children(crate::fixture_control_targets::marker(
                                                            || format!("sidebar-todos:{}", todo_session.session_id),
                                                            || None,
                                                        ))
                                                        .when(done == total, |el| el.text_color(Theme::global().OK))
                                                        .rounded_full().pr(px(2.0)).cursor_pointer()
                                                        .hover(|el| el.bg(Theme::global().TOOL_BG))
                                                        .tooltip(|_, cx| cx.new(|_| remotes::HeaderTooltip("Open session todos".into())).into())
                                                        .child(div().debug_selector(move || format!("sidebar-todos-icon-{sidebar_index}")).flex_none().child(
                                                            gpui::svg().data(include_bytes!("../../../assets/icons/todo-check.svg").as_slice()).size(px(9.0)).text_color(if done == total { Theme::global().OK } else { Theme::global().TEXT_DIM })
                                                        ))
                                                        .child(div().min_w_0().truncate().child(format!("{done}/{total}"))),
                                                ))
                                                .when_some(details.map(|d| if todo_progress.is_some() { format!("· {d}") } else { d }), |line, details| {
                                                    line.child(div().min_w_0().flex_1().truncate().child(details))
                                                }),
                                        )
                                    }),
                            );
                            if let Some(workers) = nested_workers.get(&menu_session) {
                                let folded = folded_workers.get(&menu_session).copied().unwrap_or(0);
                                let expanded_lines = this.expanded_lines.clone();
                                list = list.child(sidebar_swarm::render_workers(
                                    sidebar_swarm::WorkerRowsLayout {
                                        sidebar_index,
                                        session_id: &menu_session,
                                        under_checkout,
                                        width: if this.compact_sidebar_open { SIDEBAR_WIDTH } else { this.sidebar_custom_width },
                                    },
                                    workers,
                                    folded,
                                    &mut this.swarm_marks,
                                    &expanded_lines,
                                    cx,
                                ));
                            }
                            if menu_open {
                                let menu = this.render_collection_menu(&menu_session, cx);
                                list = list.child(div().ml(px(28.0)).mr(px(crate::scrollbar::GUTTER + 8.0)).mb_1().child(menu));
                            }
                            list.into_any_element()
                        })
                    },
                )
                .size_full(),
            ));
        }

        if self.sidebar_session_layout.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_size(px(11.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(if self.connected {
                        "no previous sessions"
                    } else {
                        "loading sessions..."
                    }),
            );
        }

        div()
            .w(px(if self.compact_sidebar_open {
                SIDEBAR_WIDTH
            } else {
                self.sidebar_custom_width
            }))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .debug_selector(|| "sidebar".into())
            .on_mouse_move(cx.listener(|this, event, _, _| this.move_sidebar_gesture(event)))
            .relative()
            .when(!folders, |el| el.bg(Theme::global().HEADER_BG))
            // Folder mode leaves this transparent: the native path owns its tab.
            .child({
                let navigation = if folders {
                    self.render_sidebar_roller(fullscreen, cx)
                } else {
                    div()
                        .h(px(TITLEBAR_HEIGHT))
                        // The transparent macOS titlebar puts the traffic lights in
                        // this row. Start navigation after them; on other platforms
                        // preserve the existing 12px inset.
                        .pl(px(sidebar_header_left_padding(fullscreen)))
                        .pr_3()
                        .flex()
                        .items_end()
                        .justify_start()
                        .relative()
                        // Keep one backing tone around the folder's top-left curve.
                        // A dark navigation rectangle here pinches that curve into a notch.
                        .bg(if folders {
                            Theme::global().HEADER_BG
                        } else {
                            Theme::global().PANEL_BG
                        })
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(1.0))
                                .bg(Theme::global().PANEL_BORDER),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .justify_end()
                                // Folder mode uses the tabs' upper contour as the thumb.
                                // Reserve the same header geometry in both modes.
                                .child(self.render_sidebar_navigation_scrollbar(cx))
                                .child(
                                    div()
                                        .id("sidebar-navigation-tabs")
                                        .debug_selector(|| "sidebar-navigation-tabs".into())
                                        // Clicks here select tabs, not the Windows caption drag.
                                        .occlude()
                                        .flex_none()
                                        .h(px(34.0))
                                        .min_w_0()
                                        .flex()
                                        .items_end()
                                        .gap_1()
                                        .overflow_x_scroll()
                                        .track_scroll(&self.sidebar_navigation_scroll)
                                        .on_scroll_wheel(cx.listener(
                                            |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                                let delta =
                                                    event.delta.pixel_delta(window.line_height());
                                                let dx = if delta.x != px(0.0) {
                                                    delta.x
                                                } else {
                                                    delta.y
                                                };
                                                let handle = &this.sidebar_navigation_scroll;
                                                let x = (handle.offset().x + dx)
                                                    .clamp(-handle.max_offset().x, px(0.0));
                                                handle.set_offset(gpui::point(x, px(0.0)));
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ))
                                        .child(
                                            div()
                                                .id("sidebar-sessions-tab")
                                                .debug_selector(|| "sidebar-sessions-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Sessions,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Sessions {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Sessions;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("chat"),
                                        )
                                        .child(self.render_tutorial_tab(cx))
                                        .child(
                                            div()
                                                .id("sidebar-files-tab")
                                                .debug_selector(|| "sidebar-files-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Files,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Files {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Files;
                                                        let mut tree = this.file_tree.borrow_mut();
                                                        tree.rows.clear();
                                                        tree.revision += 1;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("files"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-accounts-tab")
                                                .debug_selector(|| "sidebar-accounts-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Accounts,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Accounts {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Accounts;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("accounts"),
                                        )
                                        .child(self.render_preference_tab(
                                            SidebarView::Theme,
                                            "theme",
                                            cx,
                                        ))
                                        .child(self.render_preference_tab(
                                            SidebarView::Settings,
                                            "settings",
                                            cx,
                                        ))
                                        .child(self.render_sidebar_search_button(cx))
                                        .child(
                                            div()
                                                .id("sidebar-openrig")
                                                .px_2()
                                                .py_1()
                                                .rounded_full()
                                                .cursor_pointer()
                                                .text_size(px(11.))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_openrig(&OpenOpenrig, window, cx)
                                                    }),
                                                )
                                                .child("OpenRig"),
                                        )
                                        .child(
                                            crate::keyboard::press_button(
                                                div().id("sidebar-orchestration"),
                                                {
                                                    let owner = cx.weak_entity();
                                                    move |window, cx| {
                                                        let _ = owner.update(cx, |this, cx| {
                                                            this.open_orchestration(
                                                                &OpenOrchestration,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                },
                                            )
                                            .debug_selector(|| "sidebar-orchestration".into())
                                            .flex_none()
                                            .border_1()
                                            .border_color(Theme::global().PANEL_BORDER)
                                            .px_2()
                                            .py_1()
                                            .rounded_t_md()
                                            .h(px(30.0))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .text_size(px(11.0))
                                            .text_color(Theme::global().TEXT_DIM)
                                            .hover(|el| {
                                                el.bg(Theme::global().HEADER_BG)
                                                    .text_color(Theme::global().TEXT)
                                            })
                                            .child("orchestration"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-unfinished-work")
                                                .debug_selector(|| "sidebar-unfinished-work".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _event, window, cx| {
                                                        this.new_unfinished_work(
                                                            &NewUnfinishedWork,
                                                            window,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("todos"),
                                        )
                                        .child(
                                            div()
                                                .id("open-todoist")
                                                .debug_selector(|| "open-todoist".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_todoist(&OpenTodoist, window, cx)
                                                    }),
                                                )
                                                .child("todoist"),
                                        )
                                        .child(
                                            div()
                                                .id("open-gmail")
                                                .debug_selector(|| "open-gmail".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_gmail(&OpenGmail, window, cx)
                                                    }),
                                                )
                                                .child("email"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-open-folder")
                                                .debug_selector(|| "sidebar-open-folder".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _event, window, cx| {
                                                        this.open_folder(&OpenFolder, window, cx);
                                                    }),
                                                )
                                                .child("folder"),
                                        )
                                        .child(
                                            crate::keyboard::press_button(
                                                div().id("sidebar-new-session"),
                                                {
                                                    let owner = cx.weak_entity();
                                                    move |_, cx| {
                                                        let _ = owner.update(cx, |this, cx| {
                                                            this.missed("new_panel", cx);
                                                            this.open_new_session(cx);
                                                        });
                                                    }
                                                },
                                            )
                                            // Tagged so a render test can click the real button
                                            // and confirm it counts as a slow path, not as
                                            // knowledge of super-enter.
                                            .debug_selector(|| "sidebar-new-session".into())
                                            .flex_none()
                                            .border_1()
                                            .border_color(Theme::global().PANEL_BORDER)
                                            .px_2()
                                            .py_1()
                                            .rounded_t_md()
                                            .h(px(30.0))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .text_size(px(16.0))
                                            .text_color(Theme::global().TEXT_DIM)
                                            .hover(|el| {
                                                el.bg(Theme::global().HEADER_BG)
                                                    .text_color(Theme::global().TEXT)
                                            })
                                            .child("+"),
                                        ),
                                ),
                        )
                        .into_any_element()
                };
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w_0().child(navigation))
            })
            // Folder mode's header already is the destination row.
            .children((!folders).then(|| self.render_machine_switcher(None, cx)))
            .children(
                (self.sidebar_view == SidebarView::Sessions)
                    .then(|| self.session_catalog_controls("sidebar", cx).px_2().py_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .relative()
                    .debug_selector(|| "sidebar-tab-body".into())
                    .when(self.sidebar_view == SidebarView::Files, |el| {
                        el.pr(px(crate::scrollbar::GUTTER))
                    })
                    .child(match self.sidebar_view {
                        SidebarView::Sessions => list.into_any_element(),
                        SidebarView::Learn => self.render_tutorial_guides(cx),
                        SidebarView::Files => self.render_files_sidebar(cx),
                        SidebarView::Theme => self.render_theme_settings(cx),
                        SidebarView::Settings => self.render_settings(cx),
                        SidebarView::Accounts => self.render_accounts(cx).unwrap_or_else(|| {
                            div()
                                .debug_selector(|| "accounts-empty".into())
                                .p_4()
                                .text_size(px(11.0))
                                .text_color(Theme::global().TEXT_DIM)
                                .child("No connected accounts")
                                .into_any_element()
                        }),
                    })
                    .when(
                        matches!(
                            self.sidebar_view,
                            SidebarView::Sessions | SidebarView::Files
                        ),
                        |el| el.child(self.render_sidebar_scrollbar(cx)),
                    ),
            )
            .child(self.render_sidebar_account(cx))
            .into_any_element()
    }

    /// The automatic-update chip.
    ///
    /// Sparkle used to update entirely in the background, so a user running a
    /// broken build had no way to learn that a fix already existed. The chip
    /// makes each phase visible: a pulse while a check or download is in
    /// flight, and a solid, clickable prompt once a build is staged and one
    /// restart away.
    fn render_update_chip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let state = updates::current();
        let label = state.label()?;
        let actionable = state.is_actionable();
        let paired = matches!(state, updates::UpdateState::PairDrain { .. });
        let ink = if actionable {
            Theme::global().ACCENT
        } else {
            Theme::global().TEXT_DIM
        };

        let mut chip = div()
            .id("update-chip")
            // Tagged so a render test can prove the chip painted and can click
            // the real element, rather than only asserting on updater state.
            .debug_selector(|| "update-chip".into())
            .when(!paired, |chip| {
                chip.absolute()
                    .bottom(px(UPDATE_CHIP_BOTTOM))
                    .right(px(MINIMAP_RIGHT))
            })
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2p5()
            .py_1()
            .rounded_full()
            .max_w(relative(1.0))
            .bg(Theme::global().MINIMAP_BG)
            .border_1()
            .border_color(if actionable {
                Theme::global().PANEL_BORDER_FOCUS
            } else {
                Theme::global().PANEL_BORDER
            })
            .text_size(px(10.5))
            .font_family(Theme::global().FONT_MONO)
            .text_color(ink)
            .occlude()
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(label),
            );

        if let updates::UpdateState::PairDrain { blockers, retry } = state {
            if !blockers.is_empty() {
                let details = blockers.join("\n");
                chip = chip
                    .tooltip(move |_, cx| cx.new(|_| PairedUpdateTooltip(details.clone())).into());
            }
            if retry {
                chip = chip.child(
                    div()
                        .id("cancel-paired-update-retry")
                        .debug_selector(|| "cancel-paired-update-retry".into())
                        .flex_none()
                        .rounded_full()
                        .px_2()
                        .bg(Theme::global().HEADER_BG)
                        .text_color(Theme::global().TEXT)
                        .hover(|el| el.opacity(0.85))
                        .cursor_pointer()
                        .on_click(|_, _, cx| {
                            cx.stop_propagation();
                            updates::cancel_pair_retry();
                        })
                        .child("Cancel"),
                );
            }
        }

        if actionable {
            chip = chip
                .cursor_pointer()
                .hover(|el| el.text_color(Theme::global().TEXT))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|_this, _event, _window, _cx| {
                        // Source builds have no Sparkle framework, so this is a
                        // no-op there rather than a crash.
                        updates::install_now();
                    }),
                );
        }

        Some(if paired {
            div()
                .w_full()
                .h(px(PAIRED_UPDATE_ROW_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .px_3()
                .child(chip)
                .into_any_element()
        } else {
            chip.into_any_element()
        })
    }

    /// The dedicated tab fills the sidebar, with fixed-height account rows and
    /// independent scrolling when the credentials exceed the available height.
    fn render_accounts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
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
                                .tooltip(|_, cx| {
                                    cx.new(|_| {
                                        remotes::HeaderTooltip(
                                            "Review a banked usage reset. Nothing is spent until you confirm."
                                                .into(),
                                        )
                                    })
                                    .into()
                                })
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
            let login_blocks = login_rows::login_blocks(account, &mut |reset| {
                if !reset.offerable() {
                    return None;
                }
                let reset_index = offered;
                offered += 1;
                Some(make_pill(reset_index, reset.clone(), reset.pill_label()))
            })
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
                            .map(|identity| identity.display_name.clone())
                    } else {
                        reset.account_label.clone().filter(|_| many)
                    };
                    let label = match owner {
                        Some(login) => format!("{} · {login}", reset.pill_label()),
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
                                .tooltip(|_, cx| {
                                    cx.new(|_| {
                                        remotes::HeaderTooltip(
                                            "Opens Jcode pricing in your browser. Nothing is purchased here."
                                                .into(),
                                        )
                                    })
                                    .into()
                                })
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
                    let tooltip = crate::panel::usage::RowsTooltip {
                        title: format!("{} · API estimate, not a bill", report.title()),
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
                                cx.new(|_| crate::panel::usage::RowsTooltip {
                                    title: tooltip.title.clone(),
                                    rows: tooltip.rows.clone(),
                                })
                                .into()
                            })
                            .when_some(label, |el, label| {
                                el.child(
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

    fn render_preference_tab(
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

    fn render_theme_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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

    fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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

    /// The minimap: a rounded card in the top right that draws every strip to
    /// scale. Panels are proportional rectangles, the focused panel is lit, a
    /// lens shows where the camera is looking, clicking a panel jumps to it,
    /// clicking a track switches strips, and scrolling over the map pans the
    /// active strip.
    fn render_minimap(
        &self,
        viewport_w: f32,
        viewport_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let track_w = MINIMAP_WIDTH - MINIMAP_PADDING * 2.0 - MINIMAP_LABEL_WIDTH;
        let panel_track_h = MINIMAP_ROW_HEIGHT - MINIMAP_PANEL_INSET * 2.0;
        let widest = (0..STRIP_COUNT)
            .map(|row| {
                self.row_indices(row)
                    .map(|index| self.slot_width(index, viewport_w) + GAP)
                    .sum::<f32>()
                    + STRUT * 2.0
            })
            .fold(viewport_w, f32::max);
        let scale = minimap_scale(track_w, panel_track_h, viewport_w, viewport_h, widest);

        let mut card = div()
            .id("minimap")
            .debug_selector(|| "minimap".into())
            .absolute()
            .top(px(MINIMAP_TOP))
            .right(px(MINIMAP_RIGHT))
            .w(px(MINIMAP_WIDTH))
            .h(px(MINIMAP_HEIGHT))
            .p(px(MINIMAP_PADDING))
            .pl(px(MINIMAP_PADDING + MINIMAP_LABEL_WIDTH))
            .flex()
            .flex_col()
            .gap(px(MINIMAP_ROW_GAP))
            .rounded_lg()
            .bg(Theme::global().MINIMAP_BG)
            .border_1()
            .border_color(Theme::global().PANEL_BORDER)
            .occlude()
            // Scrolling over the map pans the active strip's camera, scaled
            // back up to canvas distance so the map and canvas move 1:1.
            .on_scroll_wheel(cx.listener(
                move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let row = this.active_row;
                    let delta = event.delta.pixel_delta(window.line_height());
                    let mut dx = f32::from(delta.x);
                    if dx == 0.0 {
                        dx = f32::from(delta.y);
                    }
                    if dx == 0.0 || scale <= f32::EPSILON {
                        return;
                    }
                    let total = this
                        .row_indices(row)
                        .map(|index| this.slot_width(index, viewport_w) + GAP)
                        .sum::<f32>()
                        + STRUT * 2.0;
                    let next = pan_camera(this.camera_x[row], -dx / scale, total, viewport_w);
                    if (next - this.camera_x[row]).abs() >= f32::EPSILON {
                        this.camera_x[row] = next;
                        this.camera_target[row] = next;
                        this.camera_from[row] = next;
                        this.camera_started[row] = None;
                        this.camera_dirty[row] = false;
                    }
                    cx.notify();
                },
            ));

        for row in 0..STRIP_COUNT {
            let active_row = row == self.active_row;
            let accent = Theme::global().workspace_accent(row);
            let mut track = div()
                .id(("minimap-row", row))
                .debug_selector(move || format!("minimap-row-{row}"))
                .relative()
                .h(px(MINIMAP_ROW_HEIGHT))
                .rounded(px(3.0))
                .cursor_pointer()
                .bg(Theme::global().minimap_row_background(active_row, accent))
                .hover(|el| el.bg(Theme::global().minimap_row_hover(accent)))
                .child(
                    div()
                        .id(("workspace-map-badge", row))
                        .debug_selector(move || format!("workspace-map-badge-{row}"))
                        .absolute()
                        .left(px(-MINIMAP_LABEL_WIDTH))
                        .top_0()
                        .w(px(MINIMAP_LABEL_WIDTH - 4.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                let position = this.active_position_in_row();
                                this.select_row(row, position);
                                this.focus_active(window, cx);
                                cx.notify();
                            }),
                        )
                        .bg(if active_row {
                            accent
                        } else {
                            Theme::global().MINIMAP_BG
                        })
                        .text_color(if active_row {
                            Theme::global().PANEL_BG
                        } else {
                            accent
                        })
                        .child(format!("{}", row + 1))
                        .when(active_row, |el| {
                            el.child(div().absolute().inset_0().debug_selector(move || {
                                format!("workspace-map-badge-{row}-active")
                            }))
                        }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        let position = this.active_position_in_row();
                        this.select_row(row, position);
                        this.focus_active(window, cx);
                        cx.notify();
                    }),
                );

            for index in self.row_indices(row) {
                let left = self.slot_left(index, viewport_w) * scale;
                let width = (self.slot_width(index, viewport_w) * scale - 1.0).max(2.0);
                // Panels keep the canvas aspect ratio: the height is the real
                // panel height under the same scale, so a panel taller than
                // wide on screen reads taller than wide here too.
                let height = (viewport_h * scale).clamp(3.0, panel_track_h);
                let top = MINIMAP_PANEL_INSET + (panel_track_h - height) / 2.0;
                let focused = active_row && index == self.active;
                let (state, todo_progress) = {
                    let panel = self.panel_metadata(&self.slots[index], cx);
                    let progress = panel.latest_todo_progress().and_then(|(done, total)| {
                        (total > 0).then_some(done as f32 / total as f32)
                    });
                    (panel.minimap_state, progress)
                };
                let (state_name, state_color) = match state {
                    crate::panel::MinimapSessionState::Idle => {
                        ("idle", Theme::global().MINIMAP_PANEL)
                    }
                    crate::panel::MinimapSessionState::Working => ("working", Theme::global().WARN),
                    crate::panel::MinimapSessionState::Streaming => {
                        ("streaming", Theme::global().ACCENT)
                    }
                    crate::panel::MinimapSessionState::Complete => ("complete", Theme::global().OK),
                    crate::panel::MinimapSessionState::Error => ("error", Theme::global().ERROR),
                };
                track =
                    track.child(
                        div()
                            .id(("minimap-panel", index))
                            .debug_selector(move || format!("minimap-panel-{index}"))
                            .when(cfg!(test) && focused, |el| {
                                el.child(div().absolute().size_full().debug_selector(move || {
                                    format!("minimap-panel-{index}-focused")
                                }))
                            })
                            .absolute()
                            .left(px(left))
                            .top(px(top))
                            .w(px(width))
                            .h(px(height))
                            .rounded(px(2.0))
                            .cursor_pointer()
                            .bg(if focused {
                                Theme::global().MINIMAP_PANEL_BUSY
                            } else {
                                Theme::global().MINIMAP_PANEL
                            })
                            .child(
                                div()
                                    .debug_selector(move || {
                                        format!("minimap-panel-{index}-{state_name}")
                                    })
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w_full()
                                    .h(px(2.0))
                                    .bg(state_color),
                            )
                            // The green footline is a literal completion meter for
                            // the latest todo card. Session color remains visible
                            // above it, so progress and live state do not compete.
                            .when_some(todo_progress, |panel, progress| {
                                panel.child(
                                    div()
                                        .debug_selector(move || {
                                            format!("minimap-panel-{index}-todo-progress")
                                        })
                                        .absolute()
                                        .bottom_0()
                                        .left_0()
                                        .h(px(2.0))
                                        .w(relative(progress))
                                        .bg(Theme::global().OK),
                                )
                            })
                            .hover(|el| el.bg(Theme::global().MINIMAP_PANEL_BUSY))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _event, window, cx| {
                                    cx.stop_propagation();
                                    this.set_active(index, cx);
                                    this.overview = false;
                                    this.overview_progress.set(0.0, Instant::now());
                                    this.focus_active(window, cx);
                                    cx.notify();
                                }),
                            ),
                    );
            }

            // The lens: where the camera is looking on the active strip.
            if active_row {
                let lens_left = ((self.camera_x[row] + STRUT) * scale).max(0.0);
                let lens_width = (viewport_w * scale).min(track_w - lens_left).max(3.0);
                track = track.child(
                    div()
                        .debug_selector(|| "minimap-viewport".into())
                        .absolute()
                        .left(px(lens_left))
                        .top(px(0.0))
                        .w(px(lens_width))
                        .h(px(MINIMAP_ROW_HEIGHT))
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(accent),
                );

                // A persistent pin marks the exact focused panel. Unlike the
                // viewport lens, it remains unambiguous when several panels
                // overlap the visible camera region.
                if let Some(index) = self.row_indices(row).find(|index| *index == self.active) {
                    let panel_left = self.slot_left(index, viewport_w) * scale;
                    let panel_width = (self.slot_width(index, viewport_w) * scale - 1.0).max(2.0);
                    track = track.child(
                        div()
                            .debug_selector(|| "minimap-you-pin".into())
                            .absolute()
                            .left(px(panel_left + panel_width / 2.0 - 2.0))
                            .top(px(MINIMAP_ROW_HEIGHT / 2.0 - 2.0))
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(Theme::global().TEXT),
                    );
                }

                // The gesture dot: the same focal point the canvas reticle
                // marks, mirrored onto the map at the lens center so the eye
                // can track the swipe in either place. The dark ring keeps it
                // legible even over the lit focused-panel rectangle.
                if let Some(alpha) = self.gesture_reticle_alpha(cx) {
                    let dot_left = lens_left + lens_width / 2.0 - MINIMAP_GESTURE_DOT / 2.0;
                    track = track.child(
                        div()
                            .debug_selector(|| "minimap-gesture-dot".into())
                            .absolute()
                            .left(px(dot_left))
                            .top(px((MINIMAP_ROW_HEIGHT - MINIMAP_GESTURE_DOT) / 2.0))
                            .w(px(MINIMAP_GESTURE_DOT))
                            .h(px(MINIMAP_GESTURE_DOT))
                            .rounded_full()
                            .border_2()
                            .border_color(gpui::rgba(0x000000cc))
                            .bg(Theme::global().ACCENT)
                            .opacity(alpha),
                    );
                }
            }

            card = card.child(track);
        }

        card.into_any_element()
    }

    /// A single skill row: the keys, what they do, and a bar showing how well
    /// the model believes this shortcut is known right now.
    fn render_skill_row(&self, skill: &learning::Skill, mastery: f32) -> gpui::AnyElement {
        let known = mastery >= 0.7;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .font_family(Theme::global().FONT_MONO)
                    .text_color(if known {
                        Theme::global().TEXT_DIM
                    } else {
                        Theme::global().TEXT
                    })
                    .child(skill.keys),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.0))
                    .text_color(if known {
                        Theme::global().TEXT_DIM
                    } else {
                        Theme::global().TEXT
                    })
                    .child(skill.label),
            )
            // The bar is the model's belief, not a usage count: it decays when
            // a shortcut goes unused, so it reads as "how well you know this".
            .child(
                div()
                    .w(px(56.0))
                    .h(px(4.0))
                    .flex_none()
                    .rounded_full()
                    .bg(Theme::global().PANEL_BORDER)
                    .child(
                        div()
                            .w(relative(mastery.clamp(0.02, 1.0)))
                            .h_full()
                            .rounded_full()
                            .bg(if known {
                                Theme::global().OK
                            } else {
                                Theme::global().ACCENT
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The coach view: what the user knows, what they do not, and what to learn
    /// next. This replaces a flat cheat sheet, because the useful information is
    /// which of these the user has not yet made their own.
    fn render_hints_overlay(&self, progress: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let now = learning::now();
        let overall = self.coach.overall_mastery(now);
        let next = self.coach.next_lesson(now);

        let mut card = div()
            .id("hints-card")
            .debug_selector(|| "coach-card".into())
            .relative()
            .top(px((1.0 - progress) * 12.0))
            .w(px(560.0))
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .bg(Theme::global().PANEL_BG)
            .border_1()
            .border_color(Theme::global().PANEL_BORDER_FOCUS)
            .rounded_xl()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_size(px(18.0)).child("Your workspace fluency"))
                    .child(
                        div()
                            .text_color(Theme::global().TEXT_DIM)
                            .text_size(px(11.0))
                            .child("Super+/ or F1 to close"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .text_size(px(11.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(format!("{}% learned", (overall * 100.0).round() as u32))
                    .child(format!("{} keystrokes saved", self.coach.effort_saved))
                    .child(format!("{} spent the long way", self.coach.effort_wasted)),
            );

        if let Some(next) = next {
            card = card.child(
                div()
                    .p_2p5()
                    .rounded_lg()
                    .bg(Theme::global().HEADER_BG)
                    .border_1()
                    .border_color(Theme::global().PANEL_BORDER)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .child("learn next"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .font_family(Theme::global().FONT_MONO)
                                    .text_size(px(12.0))
                                    .child(next.keys),
                            )
                            .child(div().text_size(px(12.0)).child(next.label)),
                    ),
            );
        }

        card = card.child(
            div()
                .p_2p5()
                .rounded_lg()
                .bg(Theme::global().HEADER_BG)
                .text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .child("F2 rename session · Composer: ↑/↓ history · Ctrl+K/J prompts · Ctrl+W word delete · Alt+B/F word move · Ctrl+U delete to start · Ctrl/Cmd+Z undo · Esc clear"),
        );

        for (area, rows) in self.coach.report(now) {
            card = card.child(
                div()
                    .mt_1()
                    .text_size(px(10.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(area.label()),
            );
            for (skill, mastery) in rows {
                card = card.child(self.render_skill_row(skill, mastery));
            }
        }

        card = card.child(
            div()
                .id("new-help-session")
                .mt_3()
                .p_3()
                .rounded_lg()
                .bg(Theme::global().HEADER_BG)
                .border_1()
                .border_color(Theme::global().PANEL_BORDER)
                .cursor_pointer()
                .hover(|el| el.border_color(Theme::global().ACCENT))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _event, window, cx| {
                        this.new_help_session(&NewHelpSession, window, cx);
                    }),
                )
                .child("Ask Jcode about the app")
                .child(
                    div()
                        .mt_1()
                        .text_size(px(11.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Opens a new session with the hints and bundled docs loaded into context."),
                ),
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .opacity(progress)
            .bg(gpui::rgba(0x000000b8))
            .child(card)
            .into_any_element()
    }
}

/// A stable retained boundary for the sidebar. Camera ticks notify Workspace
/// without changing its model, so the sidebar can replay its last frame while
/// the canvas moves. Real workspace, panel, global and hover changes remain
/// dependencies of this view and rebuild it normally.
struct SidebarChrome {
    workspace: gpui::WeakEntity<Workspace>,
}

fn workspace_fullscreen(window: &Window) -> bool {
    window.is_fullscreen()
        // The private screenshot window must not toggle an AppKit Space.
        || (harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_FULLSCREEN").as_deref() == Ok("1"))
}

impl Render for SidebarChrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("SidebarChrome");
        // This sidebar starts below the full-window tab row on macOS.
        let fullscreen = cfg!(target_os = "macos") || window.is_fullscreen();
        self.workspace
            .update(cx, |workspace, cx| workspace.render_sidebar(fullscreen, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
        // Single-panel and modal layouts return before rendering the version
        // footer. They still need the launch check and periodic updater.
        updates::ensure_release_check();
        let _render_scope = crate::render_stats::scope("Workspace");
        Theme::sync_window_background(window, &mut self.window_background);
        self.refresh_sidebar_panel_metadata(cx);
        if let Some(slot) = self.slots.get(self.active)
            && !slot.closing
            && self.panel_metadata(slot, cx).supports_voice()
        {
            self.last_voice_chat = Some(slot.panel.entity_id());
        }
        if self.onboarding_launch.error.is_some() {
            window.focus(&self.focus_handle, cx);
            return crate::window_caption::wrap(
                self.render_onboarding_launch_error(cx),
                window,
                true,
            );
        }
        if self.account_sign_in.visible {
            self.dump_state(window, cx);
            let content = self.render_account_sign_in(window, cx);
            let root = self.voice_modal_root(content, cx);
            return crate::window_caption::wrap(root, window, true);
        }
        if self.resume.is_some() {
            self.dump_state(window, cx);
            let root = self.render_resume(window, cx);
            return crate::window_caption::wrap(root, window, true);
        }
        if self.single_panel {
            let root = self.render_single_panel(window, cx);
            #[cfg(target_os = "macos")]
            let root = root.children(updates::render_pair_dialog(window, cx));
            return crate::window_caption::wrap(root, window, true);
        }
        self.sidebar_applets = self.render_sidebar_applets(window, cx);
        self.restore_hidden_machine_focus(window, cx);
        if self.show_sidebar
            && self.layout_mode == crate::config::LayoutMode::Normal
            && self.sidebar_navigation_scroll.bounds().size.width == px(0.0)
        {
            let workspace = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = workspace.update(cx, |_, cx| cx.notify());
            });
        }
        if self.accounts_layout_pending && !self.accounts.is_empty() {
            self.accounts_layout_pending = false;
            // Overlay geometry becomes available after layout. Request just one
            // follow-up frame per account snapshot, not a scrollbar polling loop.
            let workspace = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = workspace.update(cx, |_, cx| cx.notify());
            });
        }
        let render_started = Instant::now();
        let viewport = window.viewport_size();
        let compact = responsive::is_compact(f32::from(viewport.width));
        if !compact || !self.show_sidebar {
            self.compact_sidebar_open = false;
        }
        let drawer_duration = if cx.reduce_motion() || !compact || !self.show_sidebar {
            std::time::Duration::ZERO
        } else {
            transition::policy(Transition::Overlay).duration
        };
        let drawer_progress = self.compact_sidebar_motion.update(
            self.compact_sidebar_open,
            render_started,
            drawer_duration,
        );
        if self.compact_sidebar_motion.is_animating() {
            window.request_animation_frame();
        }
        let sidebar_chrome = self
            .sidebar_chrome
            .get_or_insert_with(|| {
                let workspace = cx.entity().downgrade();
                cx.new(|_| SidebarChrome { workspace })
            })
            .clone();
        let sidebar_width = if self.show_sidebar && !compact {
            self.sidebar_custom_width
        } else {
            0.0
        };
        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        // Compact windows spend every pixel on the session: no sidebar column,
        // no connector gutter, and no decorative right margin.
        let connector_width = if self.show_sidebar && folders && !compact {
            FOLDER_CONNECTOR_WIDTH
        } else {
            0.0
        };
        let right_margin = if folders && !compact {
            FOLDER_RIGHT_MARGIN
        } else {
            0.
        };
        let files_width = self.files_width(f32::from(viewport.width));
        let canvas_w = (f32::from(viewport.width)
            - sidebar_width - connector_width - right_margin
            - if compact { 0.0 } else { files_width })
            .max(0.0);
        let viewport_w = canvas_w.max(1.0);
        if self.last_canvas_width != Some(viewport_w) {
            self.last_canvas_width = Some(viewport_w);
            self.camera_dirty.fill(true);
            self.camera_snap_pending.fill(true);
        }
        let fullscreen = workspace_fullscreen(window);
        let paired_draining = matches!(updates::current(), updates::UpdateState::PairDrain { .. });
        let viewport_h = (f32::from(viewport.height)
            - if paired_draining { PAIRED_UPDATE_ROW_HEIGHT } else { 0.0 })
        .max(1.0);

        let now = Instant::now();
        let overview_was_animating = self.overview_progress.is_animating();
        let overview_progress = self.overview_progress.sample(now);
        let overview_finished =
            overview_was_animating && !self.overview_progress.is_animating() && !self.overview;
        if self.focus_pending || (overview_finished && self.focus_handle.is_focused(window)) {
            self.focus_pending = false;
            self.focus_active(window, cx);
        }
        self.follow_keyboard_focus(window, cx);
        let hints_progress = self.hints_progress.sample(now);
        // Keep the outgoing content mounted until the fade completes.
        let coach_hint = crate::config::coaching_hints()
            .then(|| self.coach.active_hint(learning::now()))
            .flatten();
        if let Some(hint) = coach_hint {
            self.coach_display_hint = Some(hint);
        } else {
            self.coach_progress.set(0.0, now);
        }
        let mut coach_progress = self.coach_progress.sample(now);
        // Stable, offline-only visual fixture. Never extends a real notification.
        if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_NOTIFICATION").as_deref() == Ok("1")
        {
            self.coach_display_hint = Some(learning::Hint {
                skill_id: "focus_first_last",
                keys: "super-u / super-p",
                label: "Jump to either end",
                because: "Skip the extra steps. Go straight to the first or last panel.".into(),
                shown_at: learning::now(),
            });
            coach_progress = 1.0;
        }
        if coach_progress <= 0.0 && !self.coach_progress.is_animating() {
            self.coach_display_hint = None;
        }
        let row_progress = self.row_progress.sample(now);
        if !self.row_progress.is_animating() {
            self.outgoing_row = None;
        }
        let workspace_pull = self.sample_workspace_pull(now, cx);
        if self.overview_progress.is_animating()
            || self.hints_progress.is_animating()
            || self.coach_progress.is_animating()
            || self.row_progress.is_animating()
            || workspace_pull != 0.0
            || self.workspace_pull.is_animating()
            || Theme::is_transitioning()
        {
            if Theme::is_transitioning() {
                cx.refresh_windows();
            }
            window.request_animation_frame();
        }

        self.advance_hidden_strip_animations(now, overview_progress > 0.0);
        let content = if overview_progress > 0.0 {
            div()
                .size_full()
                .bg(Theme::global().BG)
                .opacity(overview_progress)
                .child(self.render_overview(cx))
                .into_any_element()
        } else if self.outgoing_row.is_some() {
            self.render_map_trip(row_progress, viewport_w, viewport_h, window, cx)
        } else if workspace_pull != 0.0 {
            self.render_workspace_pull(workspace_pull, viewport_w, viewport_h, window, cx)
        } else {
            // One path for both the populated and the empty strip, so the
            // empty strip's gesture handling exists everywhere it paints.
            self.render_row(self.active_row, viewport_w, viewport_h, window, cx)
        };

        if self.performance.is_some() {
            let frame = window.frame_duration_snapshot();
            let input = window.input_latency_snapshot();
            let milliseconds = |nanoseconds: u64| nanoseconds as f64 / 1_000_000.0;
            self.gpui_performance = GpuiPerformanceSnapshot {
                draw_p95_ms: milliseconds(frame.draw_duration_histogram.value_at_quantile(0.95)),
                present_p95_ms: milliseconds(
                    frame.present_interval_histogram.value_at_quantile(0.95),
                ),
                input_to_frame_p95_ms: milliseconds(
                    input.latency_histogram.value_at_quantile(0.95),
                ),
                input_events_per_frame_p95: input
                    .events_per_frame_histogram
                    .value_at_quantile(0.95),
                mid_draw_inputs: input.mid_draw_events_dropped,
            };
        }
        // Empty strips have no panel layout to clear camera_dirty. Their
        // navigation is settled once the row transition finishes.
        let camera_settled = self.row_indices(self.active_row).next().is_none()
            || (!self.camera_dirty[self.active_row]
                && self.camera_started[self.active_row].is_none());
        let action_settled = !self.row_progress.is_animating() && camera_settled;
        if let Some(capture) = self.action_capture.as_mut() {
            capture.observe(
                Instant::now(),
                action_settled,
                window.frame_duration_snapshot(),
                window.input_latency_snapshot(),
            );
        }
        let gpui_performance = self.gpui_performance;
        let performance = self.performance.as_ref().map(|profile| {
            let snapshot = profile.snapshot();
            let (label, color) = match snapshot.health {
                PerformanceHealth::Good => ("GOOD", gpui::rgb(0x52d273)),
                PerformanceHealth::Degraded => ("SLOW", gpui::rgb(0xf2c94c)),
                PerformanceHealth::Bad => ("BAD", gpui::rgb(0xff6b6b)),
            };
            div()
                .debug_selector(|| "selfdev-performance-profile".into())
                .absolute()
                .top(px(STRIP_PADDING_TOP + FOLDER_CONTENT_INSET + 10.0))
                .right(px(12.0))
                .rounded_md()
                .px_3()
                .py_2()
                .bg(gpui::rgba(0x111318e8))
                .border_1()
                .border_color(color)
                .text_size(px(11.0))
                .text_color(gpui::rgb(0xe5e7eb))
                .child(format!(
                    "PERF {label}  presented {:.0} fps / p95 {:.1} ms  input→frame {:.1} ms  draw {:.1} ms  view {:.1} ms / worst {:.1} ms  wake {:.1} ms  coalesced {}  mid-draw {}",
                    if gpui_performance.present_p95_ms > 0.0 {
                        1_000.0 / gpui_performance.present_p95_ms
                    } else {
                        0.0
                    },
                    gpui_performance.present_p95_ms,
                    gpui_performance.input_to_frame_p95_ms,
                    gpui_performance.draw_p95_ms,
                    snapshot.render_p95_ms,
                    snapshot.worst_ms,
                    snapshot.wake_p95_ms,
                    gpui_performance.input_events_per_frame_p95,
                    gpui_performance.mid_draw_inputs,
                ))
        });

        let tab_bar = self.render_workspace_bar(
            if cfg!(target_os = "macos") {
                f32::from(viewport.width)
            } else {
                canvas_w
            },
            compact || !self.show_sidebar,
            coach_progress,
            window,
            cx,
        );
        let (window_tab_bar, canvas_tab_bar) = if cfg!(target_os = "macos") {
            (Some(tab_bar), None)
        } else {
            (None, Some(tab_bar))
        };
        let files_pane = (files_width > 0.0).then(|| self.render_files_pane(files_width, cx));
        let (files_column, files_drawer) = if compact {
            (None, files_pane)
        } else {
            (files_pane, None)
        };
        if window_tab_bar.is_some() {
            // Navigation diagnostics expose canvas-relative coordinates.
            self.live_tabs.header_offset -= sidebar_width + connector_width;
        }
        let root = crate::keyboard::tab_navigation(div())
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(Theme::global().PANEL_BG)
            .when(folders, |el| {
                el.child(folder_surface::background(self.folder_frame.clone()))
            })
            .font_family(Theme::global().FONT_UI)
            .text_size(px(14.0 * crate::config::get().appearance.text_scale))
            .text_color(Theme::global().TEXT)
            .track_focus(&self.focus_handle)
            .capture_action(cx.listener(Self::toggle_voice))
            .capture_action(cx.listener(Self::toggle_panel_voice))
            .capture_action(cx.listener(Self::begin_voice_hold))
            .capture_action(cx.listener(Self::end_voice_hold))
            .capture_action(cx.listener(Self::begin_global_voice_hold_action))
            .capture_action(cx.listener(Self::end_global_voice_hold_action))
            .capture_key_down(cx.listener(Self::copilot_key_down))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.focus_handle.is_focused(window)
                    && !this.overview
                    && !this.overview_progress.is_animating()
                    && !this.account_sign_in.visible
                    && this.onboarding_launch.error.is_none()
                    && !this.modal_layer.read(cx).has_active()
                    && this.default_directory_picker.is_none()
                    && !this.compact_sidebar_open
                    && let Some(slot) = this.slots.get(this.active).filter(|slot| !slot.closing)
                    && slot
                        .panel
                        .update_chat(cx, |panel, cx| {
                            panel.resume_composer_typing(event, true, window, cx)
                        })
                        .unwrap_or(false)
                {
                    cx.stop_propagation();
                }
            }))
            .capture_key_up(cx.listener(Self::copilot_key_up))
            .capture_action(cx.listener(|this, _: &crate::input::Clear, window, cx| {
                if this.compact_sidebar_open {
                    this.compact_sidebar_open = false;
                    this.focus_active(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .key_context(self.key_context())
            .capture_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                // An open modal owns Cancel, and closes itself.
                if this.compact_sidebar_open && !this.modal_layer.read(cx).has_focus(window, cx) {
                    this.compact_sidebar_open = false;
                    this.focus_active(window, cx);
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            // Navigation belongs to the canvas, regardless of which control in
            // the active panel currently owns keyboard focus. Capture these at
            // the workspace boundary so a terminal, composer, picker, or other
            // focused child cannot consume the action before it reaches us.
            // Capture listeners propagate by default, so each handler stops
            // propagation to keep focused children from handling the motion.
            .capture_action(cx.listener(Self::focus_left))
            .capture_action(cx.listener(Self::focus_right))
            .capture_action(cx.listener(Self::focus_up))
            .capture_action(cx.listener(Self::focus_down))
            .capture_action(cx.listener(Self::focus_first))
            .capture_action(cx.listener(Self::focus_last))
            .capture_action(cx.listener(Self::focus_previous))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_document_file))
            .on_action(cx.listener(Self::workspace_slash_command))
            .on_action(cx.listener(Self::promote_side_question))
            .on_action(cx.listener(Self::move_panel_left))
            .on_action(cx.listener(Self::move_panel_right))
            .on_action(cx.listener(Self::move_panel_up))
            .on_action(cx.listener(Self::move_panel_down))
            .on_action(cx.listener(Self::move_panel_to_first))
            .on_action(cx.listener(Self::move_panel_to_last))
            .on_action(cx.listener(Self::open_change_review))
            .on_action(cx.listener(Self::close_change_review))
            .on_action(cx.listener(Self::new_panel))
            .on_action(cx.listener(Self::new_panel_in_pinned_directory))
            .on_action(cx.listener(Self::fork_panel))
            .capture_action(cx.listener(Self::rename_session))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::open_gmail))
            .on_action(cx.listener(Self::open_applet_showcase))
            .on_action(cx.listener(Self::open_orchestration))
            .on_action(cx.listener(Self::open_openrig))
            .on_action(cx.listener(Self::inspect_fleet_seat))
            .on_action(cx.listener(Self::open_fleet_session))
            .on_action(cx.listener(Self::open_accounts))
            .on_action(cx.listener(Self::open_usage_reset_command))
            .on_action(cx.listener(Self::publish_desktop))
            .on_action(cx.listener(Self::switch_directory))
            .on_action(cx.listener(Self::open_model_window_or_picker))
            .on_action(cx.listener(Self::open_todoist))
            .on_action(cx.listener(Self::new_unfinished_work))
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::close_panel))
            .on_action(cx.listener(Self::toggle_overview))
            .capture_action(cx.listener(Self::toggle_onboarding_simulator))
            .on_action(cx.listener(Self::toggle_hints))
            .on_action(cx.listener(Self::toggle_showcase))
            .on_action(cx.listener(Self::toggle_sidebar))
            .capture_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::cycle_theme))
            .on_action(cx.listener(Self::new_help_session))
            .on_action(cx.listener(Self::open_changelog))
            .on_action(cx.listener(Self::open_resume))
            .on_action(cx.listener(Self::open_session_search))
            .on_action(cx.listener(Self::cycle_width))
            .on_action(cx.listener(Self::maximize_width))
            .on_action(cx.listener(|this, _: &WidthPreset1, _w, cx| this.set_width(0.25, cx)))
            .on_action(cx.listener(|this, _: &WidthPreset2, _w, cx| this.set_width(0.5, cx)))
            .on_action(cx.listener(|this, _: &WidthPreset3, _w, cx| this.set_width(0.75, cx)))
            .on_action(cx.listener(|this, _: &WidthPreset4, _w, cx| this.set_width(1.0, cx)))
            .child(
                div()
                    .debug_selector(|| "workspace-body".into())
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .when(self.show_sidebar && !compact, |root| {
                        root.child(if cfg!(target_os = "macos") {
                            div()
                                .h_full()
                                .flex_none()
                                .flex()
                                .flex_col()
                                .pt(px(STRIP_PADDING_TOP + FOLDER_CONTENT_INSET))
                                .child(sidebar_chrome)
                                .into_any_element()
                        } else {
                            sidebar_chrome.into_any_element()
                        })
                        .child(self.render_sidebar_resize_handle(cx))
                    })
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .pl(px(connector_width))
                            .pr(px(right_margin))
                            .child(
                                div()
                                    .debug_selector(|| "workspace-canvas".into())
                                    .relative()
                                    .w_full()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .capture_pinch(cx.listener(Self::release_map_pinch))
                                    .on_pinch(cx.listener(Self::handle_map_pinch))
                                    .when(folders, |el| {
                                        el.child(folder_surface::measure(
                                            self.folder_frame.clone(),
                                            folder_surface::Region::Canvas,
                                        ))
                                    })
                                    .child(content)
                                    .children(canvas_tab_bar)
                                    .when(
                                        self.show_minimap
                                            && live_tabs::minimap_fits_header(canvas_w)
                                            && !self.slots.is_empty()
                                            && overview_progress <= 0.0,
                                        |el| {
                                            el.child(
                                                self.render_minimap(viewport_w, viewport_h, cx),
                                            )
                                        },
                                    )
                                    // Showcase is independent of the Learn sidebar: every
                                    // tutorial shortcut gets a visible keybinding/action cue.
                                    .when_some(self.showcase_cue.as_ref(), |el, cue| {
                                        el.child(self.render_showcase_cue(cue, window))
                                    }),
                            ),
                    )
                    .when_some(files_column, |body, files| {
                        body.child(div().h_full().flex_none().pt(px(if cfg!(target_os = "macos") {
                            STRIP_PADDING_TOP + FOLDER_CONTENT_INSET
                        } else { 0.0 })).child(files))
                    }),
            )
            .children(window_tab_bar)
            .when_some(files_drawer, |root, files| {
                root.child(div().absolute().top(px(STRIP_PADDING_TOP + FOLDER_CONTENT_INSET))
                    .bottom_0().right_0().child(files))
            })
            .when(
                compact && self.show_sidebar && self.compact_sidebar_open,
                |root| {
                    root.child(self.render_compact_sidebar_overlay(fullscreen, drawer_progress, cx))
                },
            )
            .when_some(performance, |root, performance| root.child(performance))
            .when(hints_progress > 0.0, |root| {
                root.child(self.render_hints_overlay(hints_progress, cx))
            })
            .child(self.modal_layer.clone())
            .children(self.render_applet_overlays(window, cx))
            .children(self.render_applet_consent(cx))
            .children(self.render_applet_toast(cx))
            .when(self.sidebar_resizing, |root| {
                root.child(self.render_sidebar_resize_overlay(cx))
            })
            .when(self.files_pane.resizing, |root| {
                root.child(self.render_files_resize_overlay(cx))
            });
        self.dump_state(window, cx);
        let animation_active = self.animation_active();
        let action_capture_pending = self
            .action_capture
            .as_ref()
            .is_some_and(ActionCapture::is_pending);
        if animation_active || action_capture_pending {
            self.ensure_animation_tick(cx);
        }
        if let Some(profile) = self.performance.as_mut() {
            profile.observe_render(render_started.elapsed());
            profile.observe_frame(Instant::now(), animation_active);
        }
        let root = root.when(paired_draining, |root| {
            root.children(self.render_update_chip(cx))
        });
        #[cfg(target_os = "macos")]
        let root = root.children(updates::render_pair_dialog(window, cx));
        // The workspace tab row shares the caption row, Windows Terminal style.
        crate::window_caption::wrap(root, window, false)
    }
}

struct PairedUpdateTooltip(String);

impl Render for PairedUpdateTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .max_w(px(360.0))
            .px_3()
            .py_2()
            .rounded_xl()
            .shadow_md()
            .bg(theme.PANEL_BG)
            .text_color(theme.TEXT)
            .text_size(px(12.0))
            .child(self.0.clone())
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn sidebar_enabled(
    arguments: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    configured_default: bool,
) -> bool {
    configured_default
        && jcode_desktop_api::LaunchMode::from_args(arguments)
            == jcode_desktop_api::LaunchMode::Workspace
}

/// The process default, for callers without a window (tests, helpers).
#[cfg(test)]
fn default_working_dir() -> Option<String> {
    // Match `Workspace::for_test`, whose launch is empty. Reading this process's
    // environment made the expected default follow JCODE_DESKTOP_WORKING_DIR,
    // which sessions launched from Desktop inherit, so home-directory
    // assertions failed when tests ran from inside Desktop.
    launch_working_dir(&jcode_desktop_api::WindowLaunch::default())
}

/// `JCODE_DESKTOP_WORKING_DIR` from the launch that opened a window, so each
/// window of a shared host keeps the directory its shortcut asked for.
fn launch_working_dir(launch: &jcode_desktop_api::WindowLaunch) -> Option<String> {
    launch_directory_override(launch).or_else(crate::platform_dirs::home_string)
}

fn launch_directory_override(launch: &jcode_desktop_api::WindowLaunch) -> Option<String> {
    default_working_dir_from(
        launch.var("JCODE_DESKTOP_WORKING_DIR").map(str::to_owned),
        None,
        Path::is_dir,
    )
}

impl Workspace {
    fn default_working_dir(&self) -> Option<String> {
        launch_working_dir(&self.launch)
    }

    fn new_session_directory(&self) -> Option<String> {
        self.pinned_working_dir
            .as_deref()
            .filter(|dir| Path::new(dir).is_absolute() && Path::new(dir).is_dir())
            .map(str::to_owned)
            .or_else(|| self.default_working_dir())
    }
}

fn default_working_dir_from(
    override_dir: Option<String>,
    home: Option<String>,
    is_dir: impl FnOnce(&Path) -> bool,
) -> Option<String> {
    override_dir
        .filter(|directory| {
            let path = Path::new(directory);
            path.is_absolute() && is_dir(path)
        })
        .or(home)
}

#[cfg(test)]
mod default_working_dir_tests {
    use super::default_working_dir_from;

    #[test]
    fn existing_absolute_override_takes_precedence() {
        for home in [Some("/home/user".to_owned()), None] {
            assert_eq!(
                default_working_dir_from(Some("/workspace".to_owned()), home, |_| true),
                Some("/workspace".to_owned())
            );
        }
    }

    #[test]
    fn missing_empty_or_relative_override_retains_home() {
        for directory in [None, Some(""), Some("relative"), Some("~/project")] {
            assert_eq!(
                default_working_dir_from(
                    directory.map(str::to_owned),
                    Some("/home/user".to_owned()),
                    |_| panic!("only absolute overrides should query the filesystem"),
                ),
                Some("/home/user".to_owned())
            );
        }
    }

    #[test]
    fn nonexistent_directory_or_file_retains_home_unchanged() {
        for home in [None, Some(String::new()), Some("relative-home".to_owned())] {
            assert_eq!(
                default_working_dir_from(Some("/not-a-directory".to_owned()), home.clone(), |_| {
                    false
                }),
                home
            );
        }
    }

    /// Windows of a shared single-panel host each keep the directory, args
    /// and state path of their own launch, including across a reload snapshot.
    #[test]
    fn each_window_launch_keeps_its_own_directory_and_arguments() {
        use super::{LaunchSnapshot, launch_working_dir};
        use jcode_desktop_api::{LaunchMode, WindowLaunch};
        let alpha = tempfile::tempdir().unwrap();
        let beta = tempfile::tempdir().unwrap();
        let launch = |dir: &std::path::Path, args: &[&str]| {
            WindowLaunch::from_parts(
                args.iter().copied(),
                [
                    (
                        "JCODE_DESKTOP_WORKING_DIR".into(),
                        dir.display().to_string(),
                    ),
                    (
                        "JCODE_DESKTOP_STATE".into(),
                        format!("{}/state", dir.display()),
                    ),
                ],
            )
        };
        let first = launch(alpha.path(), &["--single-panel"]);
        let second = launch(beta.path(), &["--single-panel", "--resume"]);
        assert_eq!(launch_working_dir(&first).as_deref(), alpha.path().to_str());
        assert_eq!(launch_working_dir(&second).as_deref(), beta.path().to_str());
        assert!(!LaunchMode::resume_requested(&first.args));
        assert!(LaunchMode::resume_requested(&second.args));
        let restored: WindowLaunch = LaunchSnapshot::from(&second).into();
        assert_eq!(restored, second);
        assert_eq!(
            launch_working_dir(&restored).as_deref(),
            beta.path().to_str()
        );
    }
}

/// Count session directories once. Lexical ties make the initial choice stable
/// even if the runtime returns the same history in a different order.
fn most_used_working_dir(sessions: &[jcode_sdk::SessionInfo]) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for directory in sessions
        .iter()
        .filter(|session| !harness::is_remote_session(&session.session_id))
        .filter_map(|session| session.working_dir.as_deref())
    {
        if Path::new(directory).is_absolute() && Path::new(directory).is_dir() {
            *counts.entry(directory).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .min_by_key(|(directory, count)| (std::cmp::Reverse(*count), *directory))
        .map(|(directory, _)| directory.to_owned())
}

fn filesystem_root() -> PathBuf {
    PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
}

struct FileTree {
    fallback: PathBuf,
    root: Option<PathBuf>,
    rows: HashMap<PathBuf, (Instant, std::sync::Arc<[FileRow]>)>,
    dirty: HashSet<PathBuf>,
    revision: u64,
    reading: Option<PathBuf>,
    needs_frame: bool,
}

impl Default for FileTree {
    fn default() -> Self {
        Self {
            fallback: std::env::current_dir().unwrap_or_else(|_| filesystem_root()),
            root: None,
            rows: HashMap::new(),
            dirty: HashSet::new(),
            revision: 0,
            reading: None,
            needs_frame: false,
        }
    }
}

#[derive(PartialEq, Eq)]
struct FileRow {
    path: PathBuf,
    label: gpui::SharedString,
    is_dir: bool,
}

fn read_file_rows(directory: &Path) -> std::sync::Arc<[FileRow]> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Default::default();
    };
    let mut rows = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if jcode_desktop_model::context_files::hidden_explorer_entry(&name) {
                return None;
            }
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            Some((
                (!is_dir, name.to_ascii_lowercase()),
                FileRow {
                    path: entry.path(),
                    label: label.into_owned().into(),
                    is_dir,
                },
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|(key, _), (other, _)| key.cmp(other));
    rows.into_iter().map(|(_, row)| row).collect()
}

fn contains_dot_directory(path: &Path, base: &Path) -> bool {
    path.strip_prefix(base).unwrap_or(path).components().any(|component| {
        matches!(component, std::path::Component::Normal(name) if name.to_string_lossy().starts_with('.'))
    })
}

fn directory_entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if entry.file_name().to_string_lossy().starts_with('.') {
                return None;
            }
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
    Ok(entries)
}

fn ranked_folder_matches_for_sessions(
    sessions: &[jcode_sdk::SessionInfo],
    base: &Path,
    query: &str,
) -> Vec<(PathBuf, String)> {
    let mut usage: HashMap<PathBuf, (usize, usize)> = HashMap::new();
    for (recency, session) in sessions.iter().rev().enumerate() {
        let Some(path) = session
            .working_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| {
                path != base
                    && path.starts_with(base)
                    && !contains_dot_directory(path, base)
                    && path.is_dir()
            })
        else {
            continue;
        };
        let entry = usage.entry(path).or_insert((0, recency));
        entry.0 += 1;
        entry.1 = entry.1.min(recency);
    }

    let common_names = [
        "projects",
        "code",
        "dev",
        "src",
        "workspace",
        "documents",
        "desktop",
    ];
    let children = directory_entries(base).unwrap_or_default();
    let mut reasons = HashMap::<PathBuf, String>::new();
    let mut candidates = usage.keys().cloned().collect::<Vec<_>>();
    candidates.extend(children);
    if !query.is_empty()
        && let Ok(entries) = std::fs::read_dir(base)
    {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(query)
            {
                reasons.insert(
                    base.to_path_buf(),
                    format!("contains file · {}", entry.file_name().to_string_lossy()),
                );
                candidates.push(base.to_path_buf());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.retain(|path| {
        let searchable = path.to_string_lossy().to_lowercase();
        if !query.is_empty() {
            searchable.contains(query) || reasons.contains_key(path)
        } else {
            // An empty search is a real filesystem browser, not only a list of
            // guesses. Keeping every direct child lets the user walk from home
            // to its parent and all the way to the filesystem root.
            true
        }
    });
    candidates.sort_by_key(|path| {
        let file_match = reasons.contains_key(path);
        let (count, recency) = usage.get(path).copied().unwrap_or((0, usize::MAX));
        let likely = path.file_name().is_some_and(|name| {
            common_names.contains(&name.to_string_lossy().to_lowercase().as_str())
        });
        (
            std::cmp::Reverse(file_match),
            std::cmp::Reverse(count),
            recency,
            std::cmp::Reverse(likely),
            path.clone(),
        )
    });
    candidates
        .into_iter()
        .take(if query.is_empty() { 200 } else { 30 })
        .map(|path| {
            let reason = reasons
                .remove(&path)
                .unwrap_or_else(|| match usage.get(&path).copied() {
                    Some((count, _)) if count > 1 => format!("frequent · {count} sessions"),
                    Some(_) => "recent".into(),
                    None if path.file_name().is_some_and(|name| {
                        common_names.contains(&name.to_string_lossy().to_lowercase().as_str())
                    }) =>
                    {
                        "likely".into()
                    }
                    None => String::new(),
                });
            (path, reason)
        })
        .collect()
}

/// Inputs that can change a virtual row's height, including its section header
/// and saved divider. Titles stay in a fixed-height box and do not belong here.
#[derive(Debug, PartialEq, Eq)]
struct SidebarSessionLayout {
    session_id: String,
    open: bool,
    project: usize,
    header: bool,
    checkout_header: bool,
    divider: bool,
    section: Option<&'static str>,
    collapsed: bool,
    selected: bool,
    saved: bool,
    details: bool,
    /// The row's "Move to collection" menu is open beneath it.
    menu: bool,
    /// Nested worker rows (names and states) change the item height.
    workers: String,
}

struct SidebarRow {
    /// Local panel membership drives selection and navigation.
    open: bool,
    /// A local panel or an externally connected user client, not a loaded agent.
    live: bool,
    session: jcode_sdk::SessionInfo,
    project: usize,
    subdirectory: Option<String>,
    header: bool,
    collapsed: bool,
    divider: bool,
    checkout: Option<sidebar_projects::Checkout>,
    /// First thread of a checkout, which carries its branch row.
    checkout_header: bool,
    /// "Live" or "Past" caption above the first row of each group.
    section: Option<&'static str>,
    /// Nested swarm agents: (total, working).
    agents: (usize, usize),
}

fn sync_sidebar_session_layout(
    state: &gpui::ListState,
    previous: &mut Vec<SidebarSessionLayout>,
    next: Vec<SidebarSessionLayout>,
) {
    if *previous == next {
        return;
    }
    let old_len = previous.len();
    let new_len = next.len();
    if old_len != new_len {
        let shared = old_len.min(new_len);
        state.splice(shared..old_len, new_len - shared);
    }
    // A changed row can also add/remove the next row's heading or divider.
    // Appending/removing history also moves the final row's bottom padding.
    let changed = previous.iter().zip(&next).position(|(old, new)| old != new);
    let boundary = (old_len != new_len).then(|| old_len.min(new_len).saturating_sub(1));
    if let Some(first) = changed.into_iter().chain(boundary).min() {
        state.remeasure_items(first..new_len);
    }
    // Hints give the scrollbar a useful initial size without measuring history.
    state.clone().with_uniform_item_height(px(46.0));
    *previous = next;
}

fn sidebar_session_title(session: &jcode_sdk::SessionInfo) -> (&'static str, String) {
    sidebar_session_title_with_open_title(session, None)
}

fn sidebar_session_title_with_open_title(
    session: &jcode_sdk::SessionInfo,
    open_title: Option<&str>,
) -> (&'static str, String) {
    let animal = jcode_core::id::extract_session_name(&session.session_id);
    let icon = animal.map(jcode_core::id::session_icon).unwrap_or("💫");
    // An open panel owns the tab title, including provisional first-prompt titles
    // that have not reached the session catalog yet.
    let title = open_title
        .or(session.title.as_deref())
        .or(session.agent_label.as_deref())
        .unwrap_or_default();
    (
        icon,
        crate::panel::folder_session_title(&session.session_id, title).to_string(),
    )
}

#[cfg(test)]
fn sidebar_session_order(sessions: &[jcode_sdk::SessionInfo]) -> Vec<jcode_sdk::SessionInfo> {
    let mut ordered = sessions.to_vec();
    sort_sidebar_sessions(&mut ordered, sidebar_session_catalog::Sort::Recent);
    ordered
}

fn sort_sidebar_sessions(
    ordered: &mut [jcode_sdk::SessionInfo],
    sort: sidebar_session_catalog::Sort,
) {
    match sort {
        sidebar_session_catalog::Sort::Recent => ordered.sort_by_key(sidebar_session_sort_key),
        sidebar_session_catalog::Sort::AgentActivity => ordered.sort_by_key(|session| {
            (
                std::cmp::Reverse(session.last_agent_activity_at_ms()),
                sidebar_session_sort_key(session),
            )
        }),
    }
}

fn sidebar_session_sort_key(session: &jcode_sdk::SessionInfo) -> std::cmp::Reverse<(bool, i64)> {
    std::cmp::Reverse((session.saved, sidebar_session_recency_ms(session)))
}

fn same_session_catalog(a: &[jcode_sdk::SessionInfo], b: &[jcode_sdk::SessionInfo]) -> bool {
    if a == b {
        return true;
    }
    if a.len() != b.len() || !a.iter().zip(b).all(|(a, b)| same_session_row(a, b)) {
        return false;
    }
    // Only timestamps changed. Avoid invalidating views unless their stable
    // newest-first order changes. Compare borrowed rows, not transcript data.
    let mut a: Vec<_> = a.iter().collect();
    let mut b: Vec<_> = b.iter().collect();
    a.sort_by_key(|session| sidebar_session_sort_key(session));
    b.sort_by_key(|session| sidebar_session_sort_key(session));
    a.into_iter()
        .zip(b)
        .all(|(a, b)| a.session_id == b.session_id)
}

fn same_session_row(a: &jcode_sdk::SessionInfo, b: &jcode_sdk::SessionInfo) -> bool {
    let jcode_sdk::SessionInfo {
        user_connected,
        edit_stats,
        context,
        session_id,
        parent_session_id,
        agent_label,
        swarm_status,
        working_dir,
        title,
        status,
        transcript_bytes,
        saved,
        save_label,
        updated_at_ms: _,
        last_active_at_ms,
        archived,
        archived_at_ms,
    } = a;
    *user_connected == b.user_connected
        && edit_stats == &b.edit_stats
        && context == &b.context
        && session_id == &b.session_id
        && parent_session_id == &b.parent_session_id
        && agent_label == &b.agent_label
        && swarm_status == &b.swarm_status
        && working_dir == &b.working_dir
        && title == &b.title
        && status == &b.status
        && transcript_bytes == &b.transcript_bytes
        && *saved == b.saved
        && save_label == &b.save_label
        && last_active_at_ms == &b.last_active_at_ms
        && *archived == b.archived
        && archived_at_ms == &b.archived_at_ms
}

fn sidebar_session_recency_ms(session: &jcode_sdk::SessionInfo) -> i64 {
    session
        .updated_at_ms
        .or_else(|| {
            sidebar_session_created_ms(&session.session_id)
                .and_then(|value| i64::try_from(value).ok())
        })
        .unwrap_or_default()
}

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn sidebar_session_directory(session: &jcode_sdk::SessionInfo) -> Option<String> {
    if let Some(host) = harness::remote_host(&session.session_id) {
        let host = remotes::machine_label(&host);
        return Some(
            match session
                .working_dir
                .as_deref()
                .filter(|dir| !dir.trim().is_empty())
            {
                Some(dir) => format!("{host} · {dir}"),
                None => host.to_owned(),
            },
        );
    }
    session
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|directory| !directory.is_empty())
        .map(compact_working_dir)
}

/// Creation timestamp embedded in modern session ids (`session_fox_<ms>_...`).
fn sidebar_session_created_ms(session_id: &str) -> Option<u64> {
    session_id
        .split('_')
        .filter_map(|part| part.parse::<u64>().ok())
        .find(|value| (1_000_000_000_000..10_000_000_000_000).contains(value))
}

/// `3m ago` / `2h ago` / `5d ago`, matching the TUI picker's time labels.
fn format_time_ago(created_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(created_ms) / 1_000;
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d ago");
    }
    if days < 30 {
        return format!("{}w ago", days / 7);
    }
    format!("{}mo ago", days / 30)
}

/// `~1.2k tok`, matching the TUI picker's token display. The desktop only has
/// the stored transcript size, so estimate at ~4 bytes per token.
fn format_estimated_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return format!("{tokens} tok");
    }
    const UNITS: &[(f64, &str)] = &[(1.0, ""), (1_000.0, "k"), (1_000_000.0, "M")];
    let value = tokens as f64;
    let mut index = 0;
    while index + 1 < UNITS.len() && value >= UNITS[index + 1].0 {
        index += 1;
    }
    let scaled = value / UNITS[index].0;
    if scaled >= 100.0 {
        format!("{:.0}{} tok", scaled, UNITS[index].1)
    } else {
        format!("{:.1}{} tok", scaled, UNITS[index].1)
    }
}

/// Opacity for a working session's sidebar title, a gentle cosine breath.
fn sidebar_title_pulse(phase: f32) -> f32 {
    0.75 + 0.25 * (phase * std::f32::consts::TAU).cos()
}

/// The TUI picker's per-session metadata line, from the fields the desktop
/// API actually carries: `12m ago · ~4.2k tok`.
fn sidebar_session_meta(session: &jcode_sdk::SessionInfo) -> Option<String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let created = sidebar_session_created_ms(&session.session_id)
        .map(|created_ms| format_time_ago(created_ms, now_ms));
    session_meta(session, created)
}

fn session_meta(session: &jcode_sdk::SessionInfo, age: Option<String>) -> Option<String> {
    let tokens = session
        .transcript_bytes
        .filter(|bytes| *bytes > 0)
        .map(|bytes| format_estimated_tokens(bytes / 4));
    match (age, tokens) {
        (Some(age), Some(tokens)) => Some(format!("{age} · {tokens}")),
        (age, tokens) => age.or(tokens),
    }
}

fn compact_working_dir(path: &str) -> String {
    match crate::platform_dirs::tilde(path) {
        home if home == "~" => "~ (home)".into(),
        shortened => shortened,
    }
}

/// The first panel owns the viewport. Later panels use the normal column width
/// so opening one reveals the scrolling layout rather than covering it.
fn spawned_panel_width(existing_panels: usize) -> f32 {
    if existing_panels == 0 {
        1.0
    } else {
        DEFAULT_WIDTH
    }
}

/// A lone full-width panel is only full width because it had the viewport to
/// itself. When a second panel joins it, it gives up the extra space so the
/// pair sits side by side at the default width. A width the user chose
/// deliberately (anything other than full) is left alone.
fn demoted_width(width: f32, panels_after_spawn: usize) -> f32 {
    if panels_after_spawn == 2 && width >= 1.0 {
        DEFAULT_WIDTH
    } else {
        width
    }
}

/// A one-line description of the strip layout, for tests and for the
/// `JCODE_DESKTOP_STATE` debug dump: `strip=<row> focus=<pos> widths=a,b,c`.
fn describe_strip(widths: &[f32], focus_position: Option<usize>, row: usize) -> String {
    let widths = widths
        .iter()
        .map(|w| format!("{w:.2}"))
        .collect::<Vec<_>>()
        .join(",");
    let focus = focus_position
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    format!("strip={row} focus={focus} widths={widths}")
}

/// The coach's observable state, for the diagnostic dump.
fn describe_coach(mastery: f32, saved: u32, wasted: u32, teaching: Option<&'static str>) -> String {
    format!(
        "coach mastery={:.2} saved={saved} wasted={wasted} teaching={}",
        mastery,
        teaching.unwrap_or("-")
    )
}

/// Shared cubic ease-out used by the strip camera.
fn ease_out_cubic(t: f32) -> f32 {
    transition::ease_out_cubic(t)
}

/// Where a newly created panel is inserted: directly right of the focused
/// panel when it is on this strip, otherwise after the strip's last panel.
/// `row_last` is the index of the strip's rightmost panel, if any.
/// A click that jumps this many panels or more, landing on an end of the strip,
/// is the work `super-home`/`super-end` exists for. Shorter hops are ordinary
/// left/right navigation.
const LONG_HOP: usize = 3;

/// The chord that closes the focused panel. Cmd/Super+Q everywhere; macOS also
/// accepts Cmd+W, and quitting the app moved to Cmd+Shift+Q.
#[cfg(test)]
const CLOSE_PANEL_CHORD: &str = "super-q";

/// Normalizes a chord's platform modifier so advertised catalog chords compare
/// equal to what GPUI reports on the running platform. GPUI prints the platform
/// modifier as "cmd-" on macOS and "super-" on Linux for the same binding.
#[cfg(test)]
fn platform_chord(chord: &str) -> String {
    chord.replace("cmd-", "super-").replace("win-", "super-")
}

/// Whether a catalog `keys` string shows the given chord to the user. The
/// catalog writes chords for humans ("super-h / super-l", "super-1 .. super-4"),
/// so a range is expanded to the concrete keys it stands for.
#[cfg(test)]
fn advertises(keys: &str, chord: &str) -> bool {
    for part in keys.split('/').map(str::trim) {
        if part == chord {
            return true;
        }
        // "super-1 .. super-4" advertises every key in that inclusive range.
        if let Some((start, end)) = part.split_once("..") {
            let (start, end) = (start.trim(), end.trim());
            if let (Some(first), Some(last), Some(wanted)) = (
                start.rsplit('-').next().and_then(|d| d.parse::<u32>().ok()),
                end.rsplit('-').next().and_then(|d| d.parse::<u32>().ok()),
                chord.rsplit('-').next().and_then(|d| d.parse::<u32>().ok()),
            ) {
                let prefix = &start[..start.len() - 1];
                if chord.starts_with(prefix) && (first..=last).contains(&wanted) {
                    return true;
                }
            }
        }
    }
    false
}

/// Which shortcut a click-to-focus bypassed, given how far it jumped and where
/// it landed.
fn click_skill(steps: usize, landed_at: usize, strip_len: usize) -> &'static str {
    let at_end = landed_at == 0 || landed_at + 1 == strip_len;
    if steps >= LONG_HOP && at_end {
        "focus_first_last"
    } else {
        "focus_left_right"
    }
}

fn insert_index(
    active: usize,
    active_is_on_strip: bool,
    row_last: Option<usize>,
    slot_count: usize,
) -> usize {
    if active_is_on_strip {
        active + 1
    } else {
        row_last.map(|last| last + 1).unwrap_or(slot_count)
    }
}

/// After closing the panel at `closed`, niri focuses the panel that slid into
/// its place (the right neighbour), falling back to the new rightmost panel.
/// `remaining` are the strip's indices after removal.
fn focus_after_close(closed: usize, remaining: &[usize]) -> usize {
    match remaining.iter().find(|&&index| index >= closed) {
        Some(&right) => right,
        None => remaining.last().copied().unwrap_or(0),
    }
}

/// niri `switch-preset-column-width`: the next preset above `current`,
/// wrapping back to the narrowest.
fn next_preset(current: f32) -> f32 {
    PRESET_WIDTHS
        .iter()
        .copied()
        .find(|preset| *preset > current + 0.01)
        .unwrap_or(PRESET_WIDTHS[0])
}

/// niri `maximize-column`: fill the viewport, or return to the stored width.
/// Returns the new `(width, restore)` pair. A panel that is already full width
/// with nothing stored (for example after `super-4`) toggles to the default
/// width, so the key is never a no-op.
fn toggle_maximize(width: f32, restore: Option<f32>) -> (f32, Option<f32>) {
    match restore {
        Some(restore) => (restore, None),
        None if width >= 1.0 => (DEFAULT_WIDTH, None),
        None => (1.0, Some(width)),
    }
}

/// Whether a strip panel spanning `left..left + width` (strip coordinates)
/// overlaps the viewport when the strip is scrolled to `camera_x`.
fn strip_panel_visible(left: f32, width: f32, camera_x: f32, viewport: f32) -> bool {
    let start = left - camera_x;
    start < viewport && start + width > 0.0
}

/// niri `center-focused-column "never"`: scroll the least amount that brings
/// `[left, left + width]` fully into a `viewport`-wide window at `current`.
fn scroll_into_view(current: f32, left: f32, width: f32, viewport: f32) -> f32 {
    if width + STRUT * 2.0 >= viewport {
        return left - STRUT;
    }
    if left - STRUT < current {
        left - STRUT
    } else if left + width + STRUT > current + viewport {
        left + width + STRUT - viewport
    } else {
        current
    }
}

/// Two-finger pan: move the camera by `delta`, clamped to the same range the
/// keyboard camera uses, so a swipe can never fling the strip off into space.
fn pan_camera(current: f32, delta: f32, total_width: f32, viewport: f32) -> f32 {
    let max_scroll = (total_width - viewport).max(-GAP).max(-STRUT);
    (current + delta).clamp(-STRUT, max_scroll)
}

fn smoothed_camera_position(from: f32, target: f32, elapsed: Duration, duration: Duration) -> f32 {
    if elapsed >= duration {
        return target;
    }
    let t = elapsed.as_secs_f32() / duration.as_secs_f32();
    from + (target - from) * ease_out_cubic(t)
}

/// Pixels-per-canvas-pixel for the minimap: fit the widest strip (never less
/// than one viewport) into the track width, then cap the scale so a panel's
/// mapped height fits the track. One shared scale on both axes keeps every
/// rectangle at the true canvas aspect ratio, so panels taller than wide on
/// screen stay taller than wide on the map.
fn minimap_scale(
    track_width: f32,
    track_height: f32,
    viewport_w: f32,
    viewport_h: f32,
    widest_strip: f32,
) -> f32 {
    let canvas = widest_strip.max(viewport_w).max(1.0);
    (track_width / canvas).min(track_height / viewport_h.max(1.0))
}

/// Opacity of the gesture reticle a given time after the last touchpad delta:
/// fully lit through the hold window, a linear fade after, `None` once gone.
fn gesture_alpha(since_last_delta: Duration) -> Option<f32> {
    if since_last_delta <= GESTURE_HOLD {
        return Some(1.0);
    }
    let fading = since_last_delta - GESTURE_HOLD;
    if fading >= GESTURE_FADE {
        return None;
    }
    Some(1.0 - fading.as_secs_f32() / GESTURE_FADE.as_secs_f32())
}

/// Choose the panel underneath the camera's focal point after a direct pan.
/// Gaps belong to the closest adjacent panel, which avoids a dead zone and
/// makes small, high-resolution touchpad deltas behave consistently.
fn panel_at_viewport_center(
    panels: impl IntoIterator<Item = (usize, f32)>,
    camera: f32,
    viewport: f32,
) -> Option<usize> {
    let focal_x = camera + viewport / 2.0;
    let mut left = STRUT;
    let mut closest = None;
    let mut closest_distance = f32::INFINITY;

    for (index, width_fraction) in panels {
        let width = Workspace::width_for_fraction(width_fraction, viewport);
        let center = left + width / 2.0;
        let distance = (center - focal_x).abs();
        if distance < closest_distance {
            closest = Some(index);
            closest_distance = distance;
        }
        left += width + GAP;
    }
    closest
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;

#[path = "workspace_pending.rs"]
mod pending;
#[cfg(test)]
#[path = "workspace_pending_tests.rs"]
mod pending_tests;
#[path = "workspace_sounds.rs"]
mod sound_settings;
#[path = "workspace_voice_settings.rs"]
mod voice_settings;

#[cfg(test)]
#[path = "workspace_login_tests.rs"]
mod login_tests;


fn swarm_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}
