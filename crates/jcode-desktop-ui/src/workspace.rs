//! Workspace: four niri-inspired horizontal strips of session panels with a
//! smooth camera and a zoomed-out overview.
//!
//! Panels live on one of four infinite horizontal strips. Focus moves
//! left/right within a strip and up/down between strips.

#[cfg(test)]
#[path = "workspace/controls/workspace_control_tests.rs"]
mod control_tests;

#[cfg(test)]
#[path = "workspace/surfaces/workspace_file_editor_tests.rs"]
mod file_editor_tests;

#[cfg(test)]
#[path = "workspace/terminal/workspace_terminal_tests.rs"]
mod terminal_tests;

#[path = "workspace/controls/workspace_control.rs"]
mod control;

#[path = "workspace/controls/workspace_commands.rs"]
mod commands;

#[path = "workspace/sidebar/accounts/workspace_account_sign_in.rs"]
mod account_sign_in;

#[path = "workspace/applets/workspace_applets.rs"]
pub(crate) mod applets;

#[path = "workspace/applets/workspace_applet_surfaces.rs"]
mod applet_surfaces;
#[path = "workspace/layout/panels/workspace_side_panel.rs"]
mod side_panel;

#[path = "workspace/surfaces/workspace_screens.rs"]
pub(crate) mod screens;
#[path = "workspace/resume/workspace_resume.rs"]
pub(crate) mod resume;
#[path = "workspace/resume/workspace_resume_view.rs"]
mod resume_view;

#[path = "workspace/controls/workspace_menu.rs"]
mod menu;
#[path = "workspace/layout/panels/workspace_single_panel.rs"]
mod single_panel;
#[path = "workspace/lifecycle/workspace_window_launch.rs"]
mod window_launch;
#[path = "workspace/remotes/workspace_remotes_fixtures.rs"]
mod remotes_fixtures;
#[path = "workspace/lifecycle/workspace_boot.rs"]
mod boot;
#[path = "workspace/recovery/workspace_snapshot.rs"]
mod snapshot;
#[path = "workspace/lifecycle/workspace_updates.rs"]
mod runtime_updates;
#[path = "workspace/sessions/workspace_sessions.rs"]
mod sessions;
#[path = "workspace/navigation/workspace_navigation.rs"]
mod navigation;
#[path = "workspace/layout/panels/workspace_panels.rs"]
mod panels;
#[path = "workspace/controls/workspace_toggles.rs"]
mod toggles;
#[path = "workspace/layout/workspace_strip_view.rs"]
mod strip_view;
#[path = "workspace/sidebar/chrome/sidebar_view.rs"]
mod sidebar_view;
#[path = "workspace/controls/workspace_settings_view.rs"]
mod settings_view;
#[path = "workspace/layout/panels/workspace_chrome_view.rs"]
mod chrome_view;
#[path = "workspace/layout/workspace_strip_gesture.rs"]
mod strip_gesture;
#[path = "workspace/layout/workspace_strip_geometry.rs"]
mod strip_geometry;
#[path = "workspace/sidebar/catalog/workspace_sidebar_sessions.rs"]
mod sidebar_sessions;
#[path = "workspace/directories/workspace_working_dirs.rs"]
mod working_dirs;
use snapshot::*;
pub use snapshot::WorkspaceSnapshot;
use strip_gesture::*;
use strip_geometry::*;
use sidebar_sessions::*;
use working_dirs::*;

#[path = "workspace/voice/workspace_global_voice.rs"]
mod global_voice;

#[path = "workspace/layout/panels/workspace_panel_window.rs"]
pub(crate) mod panel_window;

#[path = "workspace/surfaces/workspace_preview.rs"]
mod preview;

#[path = "workspace/surfaces/workspace_changelog.rs"]
mod changelog_panel;

#[path = "workspace/surfaces/workspace_publish.rs"]
mod publish_panel;

#[path = "workspace/directories/workspace_switch_directory.rs"]
mod switch_directory;

#[path = "workspace/layout/panels/workspace_responsive.rs"]
mod responsive;
#[path = "workspace/surfaces/workspace_files.rs"]
pub(crate) mod files;

#[path = "workspace/lifecycle/workspace_notifications.rs"]
mod notifications;

#[path = "workspace/surfaces/workspace_change_review.rs"]
pub(crate) mod change_review;

#[path = "workspace/sidebar/accounts/sidebar_account_menu.rs"]
mod account_menu;
#[path = "workspace/sidebar/accounts/sidebar_jcode_account.rs"]
mod jcode_account;
#[path = "workspace/sidebar/accounts/sidebar_account.rs"]
mod sidebar_account;
#[path = "workspace/sidebar/accounts/sidebar_routing.rs"]
mod sidebar_routing;
#[path = "workspace/sidebar/catalog/sidebar_collections.rs"]
mod sidebar_collections;
#[path = "workspace/sidebar/catalog/sidebar_edits.rs"]
mod sidebar_edits;
#[path = "workspace/sidebar/chrome/sidebar_gesture.rs"]
mod sidebar_gesture;
#[path = "workspace/sidebar/chrome/sidebar_header.rs"]
mod sidebar_header;
#[cfg(test)]
#[path = "workspace/sidebar/chrome/sidebar_header_tests.rs"]
mod sidebar_header_tests;
#[path = "workspace/sidebar/catalog/workspace_sidebar_metadata.rs"]
mod sidebar_metadata;
#[path = "workspace/sidebar/catalog/sidebar_projects.rs"]
mod sidebar_projects;
#[path = "workspace/sidebar/chrome/sidebar_selection.rs"]
mod sidebar_selection;
#[path = "workspace/sidebar/catalog/sidebar_session_catalog.rs"]
mod sidebar_session_catalog;
#[path = "workspace/sidebar/catalog/sidebar_swarm.rs"]
mod sidebar_swarm;
#[path = "workspace/sidebar/catalog/sidebar_worktrees.rs"]
mod sidebar_worktrees;
#[path = "workspace/sidebar/chrome/sidebar_atoms.rs"]
mod sidebar_atoms;
#[path = "workspace/sidebar/chrome/sidebar_quick_actions.rs"]
mod sidebar_quick_actions;
#[path = "workspace/surfaces/workspace_stream_fixture.rs"]
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

#[path = "workspace/recovery/workspace_recovery.rs"]
pub(crate) mod recovery;
#[path = "workspace/lifecycle/workspace_restart.rs"]
pub(crate) mod restart;
#[path = "workspace/lifecycle/workspace_housekeeping.rs"]
mod housekeeping;

#[cfg(test)]
#[path = "workspace/navigation/closing_navigation_tests.rs"]
mod closing_navigation_tests;
#[cfg(test)]
#[path = "workspace/layout/panels/fps_header_tests.rs"]
mod fps_header_tests;
#[cfg(test)]
#[path = "workspace/layout/panels/hidden_animation_tests.rs"]
mod hidden_animation_tests;
#[path = "workspace/navigation/workspace_map_gesture.rs"]
mod map_gesture;
#[path = "workspace/navigation/workspace_map_motion.rs"]
mod map_motion;
#[cfg(test)]
#[path = "workspace/layout/gestures/workspace_nav_profile_tests.rs"]
mod nav_profile_tests;
#[cfg(test)]
#[path = "workspace/navigation/navigation_map_tests.rs"]
mod navigation_map_tests;
#[path = "workspace/navigation/navigation_state.rs"]
mod navigation_state;
#[path = "workspace/remotes/workspace_openrig.rs"]
mod openrig;
#[path = "workspace/remotes/workspace_fleet_session.rs"]
mod fleet_session;
#[cfg(test)]
#[path = "workspace/remotes/panel_cache_tests.rs"]
pub(crate) mod panel_cache_tests;
#[cfg(test)]
#[path = "workspace/remotes/workspace_remote_tests.rs"]
mod remote_tests;
#[path = "workspace/remotes/workspace_machines.rs"]
pub(crate) mod machines;
#[path = "workspace/remotes/workspace_remotes.rs"]
mod remotes;
#[path = "workspace/layout/gestures/workspace_scroll_snap.rs"]
mod scroll_snap;
#[cfg(test)]
#[path = "workspace/layout/gestures/super_action_behavior_tests.rs"]
mod super_action_behavior_tests;
#[path = "workspace/voice/workspace_voice.rs"]
mod voice;
#[path = "workspace/navigation/window_navigation.rs"]
mod window_navigation;
#[cfg(test)]
#[path = "workspace/navigation/window_navigation_tests.rs"]
mod window_navigation_tests;
#[cfg(test)]
#[path = "workspace/layout/panels/workspace_lone_panel_tests.rs"]
mod workspace_lone_panel_tests;

#[path = "workspace/sidebar/accounts/workspace_login_rows.rs"]
mod login_rows;
#[path = "workspace/sidebar/accounts/workspace_usage_reset.rs"]
mod usage_reset;

#[path = "workspace/directories/workspace_rename.rs"]
mod rename;

#[path = "workspace/directories/workspace_pin.rs"]
mod pin;

#[path = "workspace/directories/workspace_default_directory.rs"]
mod default_directory;
#[path = "workspace/directories/workspace_folder_picker.rs"]
mod folder_picker;
use folder_picker::FolderPicker;
#[cfg(test)]
#[path = "workspace/directories/workspace_default_directory_tests.rs"]
mod default_directory_tests;
#[cfg(test)]
#[path = "workspace/layout/panels/workspace_modal_tests.rs"]
mod modal_tests;

#[cfg(test)]
#[path = "workspace/resume/workspace_resume_search_tests.rs"]
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
const SHOWCASE_FADE: Duration = Duration::from_millis(280);
const SINGLE_PANEL_BREAKPOINT: f32 = 1100.0;
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

#[path = "workspace/directories/folder_surface.rs"]
mod folder_surface;

#[path = "workspace/layout/tabs/live_tabs.rs"]
mod live_tabs;
#[path = "workspace/lifecycle/workspace_version.rs"]
mod version;

#[path = "workspace/sidebar/chrome/sidebar_roller.rs"]
mod sidebar_roller;
#[path = "workspace/sidebar/chrome/sidebar_search_button.rs"]
mod sidebar_search_button;

#[cfg(test)]
#[path = "workspace/layout/panels/panel_surface_tests.rs"]
mod panel_surface_tests;

#[cfg(test)]
#[path = "workspace/layout/gestures/hover_scroll_tests.rs"]
mod hover_scroll_tests;

#[cfg(test)]
#[path = "workspace/layout/gestures/focus_follow_tests.rs"]
mod focus_follow_tests;
#[path = "workspace/remotes/workspace_pull.rs"]
mod workspace_pull;
#[cfg(test)]
#[path = "workspace/layout/gestures/workspace_scroll_tests.rs"]
mod workspace_scroll_tests;

#[cfg(test)]
#[path = "workspace/remotes/workspace_orchestration_tests.rs"]
mod workspace_orchestration_tests;

#[path = "workspace/onboarding/tutorial.rs"]
mod tutorial;

#[path = "workspace/onboarding/onboarding_simulator.rs"]
mod onboarding_simulator;

#[path = "workspace/onboarding/workspace_beta_notice.rs"]
mod beta_notice;

#[cfg(test)]
#[path = "workspace/onboarding/tutorial_geometry_tests.rs"]
mod tutorial_geometry_tests;

#[cfg(test)]
#[path = "workspace/onboarding/tutorial_minimalism_tests.rs"]
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
    remotes: machines::Machines,
    host: HostHandle,
    /// Launch-only presentation, independent of persisted workspace preferences.
    single_panel: bool,
    /// Arguments and forwarded environment of the launch that opened this
    /// window. A shared single-panel host has one per window.
    launch: jcode_desktop_api::WindowLaunch,
    resume: Option<screens::State>,
    /// Full-height global search over every stored session. Transient by
    /// design: relaunching the app lands on the workspace, not on a query.
    show_sidebar: bool,
    sidebar_custom_width: f32,
    sidebar_resizing: bool,
    // Launch-only chrome. Reload snapshots must never re-open the notice.
    account_sign_in: account_sign_in::State,
    /// The launch-only sign-in page is on screen, taking the keyboard.
    account_sign_in_visible: bool,
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
    collection_menu: sidebar_collections::RowMenu,
    /// Drafts started inside a collection: (draft request id, collection id).
    pending_collection_drafts: Vec<(String, String)>,
    tutorial_page: usize,
    onboarding_launch: screens::LaunchState,
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
    /// Sessions their machine reported gone. Their panels leave on the next
    /// render, which has the Window a panel's removal needs.
    gone_sessions: Vec<String>,
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

    // --- Geometry -------------------------------------------------------

    // --- Actions --------------------------------------------------------

    // --- Learning -------------------------------------------------------

    // --- Rendering ------------------------------------------------------

}

/// A stable retained boundary for the sidebar. Camera ticks notify Workspace
/// without changing its model, so the sidebar can replay its last frame while
/// the canvas moves. Real workspace, panel, global and hover changes remain
/// dependencies of this view and rebuild it normally.
type RenderSidebar = dyn Fn(bool, &mut App) -> gpui::AnyElement;

struct SidebarChrome {
    render_sidebar: Box<RenderSidebar>,
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
        (self.render_sidebar)(fullscreen, cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootSurface {
    OnboardingError,
    AccountSignIn,
    Resume,
    SinglePanel,
    Canvas,
}

impl Workspace {
    fn root_surface(&self) -> RootSurface {
        if self.onboarding_launch.error.is_some() {
            RootSurface::OnboardingError
        } else if self.account_sign_in_visible {
            RootSurface::AccountSignIn
        } else if self.resume.is_some() {
            RootSurface::Resume
        } else if self.single_panel {
            RootSurface::SinglePanel
        } else {
            RootSurface::Canvas
        }
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
        match self.root_surface() {
            RootSurface::OnboardingError => {
                window.focus(&self.focus_handle, cx);
                return crate::window_caption::wrap(
                    self.render_onboarding_launch_error(cx),
                    window,
                    true,
                );
            }
            RootSurface::AccountSignIn => {
                self.dump_state(window, cx);
                let content = self.render_account_sign_in(window, cx);
                let root = self.voice_modal_root(content, cx);
                return crate::window_caption::wrap(root, window, true);
            }
            RootSurface::Resume => {
                self.dump_state(window, cx);
                let root = self.render_resume(window, cx);
                return crate::window_caption::wrap(root, window, true);
            }
            RootSurface::SinglePanel => {
                let root = self.render_single_panel(window, cx);
                #[cfg(target_os = "macos")]
                let root = root.children(updates::render_pair_dialog(window, cx));
                return crate::window_caption::wrap(root, window, true);
            }
            RootSurface::Canvas => {}
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
        let compact = is_compact(f32::from(viewport.width));
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
                cx.new(|_| SidebarChrome {
                    render_sidebar: Box::new(move |fullscreen, app| {
                        workspace.update(app, |workspace, cx| workspace.render_sidebar(fullscreen, cx))
                            .unwrap_or_else(|_| div().into_any_element())
                    }),
                })
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
            self.live_tabs.shift_header(-(sidebar_width + connector_width));
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
                    && this.root_surface() == RootSurface::Canvas
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
                    })
                    // Last, so it paints and hit-tests above the canvas, its
                    // panels and the floating chrome, none of which may cover the edge.
                    .when(self.show_sidebar && !compact, |body| {
                        body.child(self.render_sidebar_resize_handle(cx))
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

#[cfg(test)]
#[path = "workspace/sessions/workspace_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workspace/lifecycle/startup_tests.rs"]
mod startup_tests;

#[path = "workspace/sessions/workspace_pending.rs"]
mod pending;
#[cfg(test)]
#[path = "workspace/sessions/workspace_pending_tests.rs"]
mod pending_tests;
#[path = "workspace/lifecycle/workspace_sounds.rs"]
mod sound_settings;
#[path = "workspace/voice/workspace_voice_settings.rs"]
mod voice_settings;

#[cfg(test)]
#[path = "workspace/sidebar/accounts/workspace_login_tests.rs"]
mod login_tests;


fn swarm_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

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

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}
