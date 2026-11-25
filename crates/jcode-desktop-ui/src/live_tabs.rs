//! Labeled live-session folder tabs, independent of the sidebar's navigation.
use super::*;
use crate::components::Tooltip;
use crate::panel::folder_session_title;
use gpui::Div;

/// Canvas air beneath the rounded tabs keeps them detached from the sheets.
const TAB_FLOAT_GAP: f32 = 4.0;
const TAB_GAP: f32 = 6.0;
const TAB_HEIGHT: f32 = FOLDER_CONTENT_INSET - TAB_FLOAT_GAP;
const TAB_HOVER_LIFT: f32 = 2.0;
const TAB_RENAME_SIZE: f32 = 20.0;
const TAB_CLOSE_PILL_SIZE: f32 = 20.0;
const TAB_CLOSE_GLYPH_SIZE: f32 = 16.0;
const TAB_STATUS_WIDTH: f32 = 88.0;
const TAB_WINDOW_DRAG_WIDTH: f32 = 16.0;
const TAB_OVERFLOW_WIDTH: f32 = 56.0;
const MIN_TAB_EXPOSURE: f32 = 16.0;
/// Compact windows and hidden sidebars need a sidebar menu button.
const TAB_MENU_WIDTH: f32 = 36.0;
const TAB_ACTION_GAP: f32 = 3.0;
const TAB_ACTION_SLOT: f32 = TAB_HEIGHT + TAB_ACTION_GAP;
const TAB_CAPTION_WIDTH: f32 = if crate::window_caption::enabled() {
    crate::window_caption::BUTTONS_WIDTH
} else {
    0.0
};
const TAB_ACTIONS_WIDTH: f32 = TAB_ACTION_SLOT
    + TAB_HEIGHT
    + if crate::window_caption::enabled() {
        TAB_CAPTION_WIDTH
    } else {
        TAB_ACTION_SLOT
    };
const TAB_GROUP_LABEL_WIDTH: f32 = 24.0;
const TAB_GROUP_GAP: f32 = 28.0;

fn fps_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("JCODE_DESKTOP_SHOW_FPS").as_deref() == Ok("1"))
}

type TabEntry = (Option<usize>, usize, usize);

fn fit_tabs(entries: &mut Vec<TabEntry>, selected: usize, available: f32) -> Vec<usize> {
    let groups = entries
        .iter()
        .filter(|(_, _, position)| *position == 0)
        .count();
    let capacity = |width: f32| {
        let width = width.max(0.0);
        let group_space = groups as f32 * group_label_width(width, groups)
            + groups.saturating_sub(1) as f32
                * TAB_GROUP_GAP.min(width / (groups.max(1) as f32 * 4.0));
        ((width - group_space) / MIN_TAB_EXPOSURE).floor() as usize
    };
    if entries.len() <= capacity(available).max(1) {
        return Vec::new();
    }
    let visible = capacity(available - TAB_OVERFLOW_WIDTH).max(1);
    // Keep the selected session and, where possible, a representative from
    // each workspace. Fill the remaining slots with the nearest neighbours.
    let active_row = entries[selected].1;
    let mut keep: Vec<_> = (0..entries.len()).collect();
    keep.sort_unstable_by_key(|&index| {
        let (_, row, position) = entries[index];
        (
            index != selected,
            position != 0 || row == active_row,
            index.abs_diff(selected),
            index,
        )
    });
    keep.truncate(visible);
    keep.sort_unstable();
    let mut keep = keep.into_iter().peekable();
    let mut position = None;
    let mut hidden = Vec::new();
    let mut index = 0;
    entries.retain_mut(|entry| {
        let visible = keep.next_if_eq(&index).is_some();
        index += 1;
        if visible {
            let row_position = position
                .filter(|(row, _)| *row == entry.1)
                .map_or(0, |(_, prev)| prev + 1);
            position = Some((entry.1, row_position));
            entry.2 = row_position;
        } else if let Some(panel) = entry.0 {
            hidden.push(panel);
        }
        visible
    });
    hidden
}

/// Secondary minimap chrome must leave a usable navigation track. This is
/// presentation-only: resizing wider restores the user's minimap preference.
pub(super) fn minimap_fits_header(canvas_width: f32) -> bool {
    canvas_width
        >= MINIMAP_WIDTH
            + MINIMAP_RIGHT
            + 8.0
            + TAB_MENU_WIDTH
            + if fps_enabled() { TAB_STATUS_WIDTH } else { 0.0 }
            + TAB_ACTIONS_WIDTH
            + TAB_ACTION_SLOT
            + 64.0
}

/// Build metadata lives at the bottom, leaving the header to navigation.
pub(super) fn version_header_width(_tab_budget: f32) -> f32 {
    0.0
}

fn tab_action_button(id: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .size(px(TAB_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .occlude()
}

fn group_label_width(available: f32, group_count: usize) -> f32 {
    TAB_GROUP_LABEL_WIDTH.min(available.max(0.0) / (group_count.max(1) as f32 * 4.0))
}

/// The floating tabs are independent of the panel camera. Selection adds a
/// small width accent and a 4px lift, so the panels do the large slide.
#[derive(Clone, Copy, Debug)]
struct TabLayout {
    start: f32,
    active_width: f32,
    inactive_width: f32,
    step: f32,
    right_step: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabGeometry {
    left: f32,
    width: f32,
    height: f32,
}

/// Stacked tabs must hide labels beneath them even in translucent themes.
fn tab_surface(mut color: gpui::Rgba) -> gpui::Rgba {
    color.a = 1.0;
    color
}

#[derive(Clone, Copy)]
struct TabCloseColors {
    background: gpui::Rgba,
    glyph: gpui::Rgba,
}

impl TabCloseColors {
    fn for_tab(ring: Option<crate::panel::TabRing>, workspace_accent: gpui::Rgba) -> Self {
        let accent = ring.map_or(workspace_accent, crate::panel::TabRing::color);
        let text = Theme::global().TEXT;
        Self {
            background: accent.opacity(0.16),
            glyph: text.blend(accent.opacity(0.75)),
        }
    }
}

/// A quiet chip uses only existing air to the right of every tab.
fn coach_chip_left(available: f32, current: &[TabGeometry], target: &[TabGeometry]) -> Option<f32> {
    let right = current
        .iter()
        .chain(target)
        .map(|tab| tab.left + tab.width)
        .fold(0.0_f32, f32::max);
    let left = available - notifications::COACH_CHIP_WIDTH - 8.0;
    (left >= right + 16.0).then_some(left)
}

impl TabLayout {
    /// Keep each workspace together, with the active workspace taking priority.
    /// `rows` is in workspace/session navigation order, including an empty
    /// active workspace's placeholder. Geometry never changes that order.
    fn grouped(available: f32, rows: &[usize], selected: usize) -> Vec<TabGeometry> {
        if rows.is_empty() {
            return Vec::new();
        }
        let available = available.max(0.0);
        let mut groups = Vec::new();
        let mut start = 0;
        while start < rows.len() {
            let end = start + rows[start..].partition_point(|row| *row == rows[start]);
            groups.push(start..end);
            start = end;
        }
        let active_group = groups
            .iter()
            .position(|group| group.contains(&selected))
            .unwrap();
        // At normal sizes there is real air between workspaces, not merely an
        // outline. On tiny tracks reserve most of the width for actual tabs.
        let gap = TAB_GROUP_GAP.min(available / (groups.len() as f32 * 4.0));
        let label_width = group_label_width(available, groups.len());
        let labels_width = label_width * groups.len() as f32;
        let content = (available - gap * (groups.len() - 1) as f32 - labels_width).max(0.0);
        let compact_ideal =
            |count: usize| (56.0 + (56.0 + TAB_GAP) * (count - 1) as f32).min(144.0);
        let compact_total: f32 = groups
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != active_group)
            .map(|(_, group)| compact_ideal(group.len()))
            .sum();
        // Even hundreds of remote sessions must not squeeze the current row
        // down to the same tiny tabs. Every remote session still gets a lip.
        let compact_budget = compact_total.min(content * 0.35);
        let active = &groups[active_group];
        let active_layout = Self::new(content - compact_budget, active.len());
        let active_used =
            active_layout.active_width + active_layout.step * (active.len() - 1) as f32;
        let mut left = 0.0;
        let mut tabs = Vec::with_capacity(rows.len());
        for (group_index, group) in groups.iter().enumerate() {
            left += label_width;
            let width = if group_index == active_group {
                active_used
            } else {
                compact_budget * compact_ideal(group.len()) / compact_total
            };
            if group_index == active_group {
                for position in 0..group.len() {
                    let mut tab = active_layout.geometry(position, selected - group.start);
                    tab.left += left - active_layout.start;
                    tabs.push(tab);
                }
            } else {
                let tab_width = if group.len() == 1 {
                    width
                } else {
                    width * 0.65
                }
                .min(56.0);
                let step = if group.len() == 1 {
                    0.0
                } else {
                    (width - tab_width) / (group.len() - 1) as f32
                };
                for position in 0..group.len() {
                    tabs.push(TabGeometry {
                        left: left + step * position as f32,
                        width: tab_width,
                        height: TAB_HEIGHT - 4.0,
                    });
                }
            }
            left += width + gap;
        }
        tabs
    }

    fn new(available: f32, count: usize) -> Self {
        let available = available.max(0.0);
        let count = count.max(1);
        let active_width = if count == 1 {
            available.min(208.0)
        } else {
            (available * 0.55).min(208.0)
        }
        .floor();
        let inactive_width = (active_width - 12.0).max(active_width * 0.9).floor();
        let step = if count == 1 {
            0.0
        } else {
            ((available - active_width) / (count - 1) as f32)
                // Leave a little air at normal widths, compressing only crowded rows.
                .min(inactive_width + TAB_GAP)
        };
        let used = active_width + step * (count - 1) as f32;
        Self {
            start: ((available - used) / 2.0).max(0.0),
            active_width,
            inactive_width,
            step,
            right_step: step,
        }
    }

    fn geometry(self, position: usize, selected: usize) -> TabGeometry {
        TabGeometry {
            left: self.start
                + self.step * position.min(selected) as f32
                + if position > selected {
                    self.active_width - self.inactive_width
                        + self.right_step * (position - selected) as f32
                } else {
                    0.0
                },
            width: if position == selected {
                self.active_width
            } else {
                self.inactive_width
            },
            height: if position == selected {
                TAB_HEIGHT
            } else {
                TAB_HEIGHT - 4.0
            },
        }
    }

    fn paint_order(count: usize, selected: usize) -> impl Iterator<Item = usize> {
        (0..selected)
            .chain((selected + 1..count).rev())
            .chain(std::iter::once(selected))
    }

    /// A hovered tab rises above every other tab, including the focused one,
    /// so it can be inspected without changing focus.
    fn hover_paint_order(count: usize, selected: usize, hovered: Option<usize>) -> Vec<usize> {
        let mut order: Vec<_> = Self::paint_order(count, selected).collect();
        if let Some(hovered) = hovered.filter(|&hovered| hovered < count) {
            order.retain(|&i| i != hovered);
            order.push(hovered);
        }
        order
    }

    /// Pop a hovered tab out: full height and at least the focused tab's width.
    /// It grows only rightward (shifting left at the track's end), so the
    /// pointer that hovered it always stays inside and hover cannot flicker.
    fn pop_out(tab: TabGeometry, focused_width: f32, available: f32) -> TabGeometry {
        let width = tab.width.max(focused_width).max(160.0).min(available);
        TabGeometry {
            left: tab.left.min(available - width).max(0.0),
            width,
            height: TAB_HEIGHT,
        }
    }

    /// Keep labels, status dots, and click targets out of the overlapping lip.
    fn exposed(tabs: &[TabGeometry], position: usize, selected: usize) -> (f32, f32) {
        let order: Vec<_> = Self::paint_order(tabs.len(), selected).collect();
        Self::exposed_in(tabs, position, &order)
    }

    fn exposed_in(tabs: &[TabGeometry], position: usize, order: &[usize]) -> (f32, f32) {
        let tab = tabs[position];
        let mut left = tab.left;
        let mut right = tab.left + tab.width;
        for other in order
            .iter()
            .copied()
            .skip_while(|&i| i != position)
            .skip(1)
            .map(|i| tabs[i])
        {
            if other.left >= right || other.left + other.width <= left {
                continue;
            }
            if other.left > left {
                right = right.min(other.left);
            } else {
                left = left.max(other.left + other.width).min(right);
            }
        }
        (left, (right - left).max(0.0))
    }
}

struct TabTween {
    target: TabGeometry,
    left: AnimatedValue,
    width: AnimatedValue,
    height: AnimatedValue,
}

impl TabTween {
    fn new(target: TabGeometry, duration: Duration) -> Self {
        Self {
            target,
            left: AnimatedValue::new(target.left, duration),
            width: AnimatedValue::new(target.width, duration),
            height: AnimatedValue::new(target.height, duration),
        }
    }

    fn sample(&mut self, now: Instant) -> TabGeometry {
        TabGeometry {
            left: self.left.sample(now),
            width: self.width.sample(now),
            height: self.height.sample(now),
        }
    }
}

#[derive(Default)]
pub(super) struct TabMotion {
    tabs: HashMap<u64, TabTween>,
    close_focus: HashMap<u64, gpui::FocusHandle>,
    available: Option<f32>,
    hit_targets: Vec<(usize, f32)>,
    header_offset: f32,
    version_width: f32,
    /// Motion key of the tab under the pointer, popped out for inspection.
    hovered: Option<u64>,
    overflow_open: bool,
    overflow_focus: Option<gpui::FocusHandle>,
}

impl TabMotion {
    fn sample(
        &mut self,
        targets: &[(u64, TabGeometry)],
        available: f32,
        duration: Duration,
        now: Instant,
    ) -> Vec<TabGeometry> {
        // A window resize must not leave animated tabs outside the new bounds.
        let snap = self.available != Some(available) || duration.is_zero();
        self.available = Some(available);
        let keys: HashSet<_> = targets.iter().map(|(key, _)| *key).collect();
        self.tabs.retain(|key, _| keys.contains(key));
        self.close_focus.retain(|key, _| keys.contains(key));
        targets
            .iter()
            .map(|(key, target)| {
                let tween = self
                    .tabs
                    .entry(*key)
                    .or_insert_with(|| TabTween::new(*target, duration));
                if snap {
                    *tween = TabTween::new(*target, duration);
                } else if tween.target != *target {
                    tween.left.set(target.left, now);
                    tween.width.set(target.width, now);
                    tween.height.set(target.height, now);
                    tween.target = *target;
                }
                tween.sample(now)
            })
            .collect()
    }

    /// The focus handle of a tab's close control, created on first use.
    fn close_focus(&mut self, key: u64, cx: &App) -> gpui::FocusHandle {
        self.close_focus
            .entry(key)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// The focus handle of the overflow trigger, created on first use.
    fn overflow_focus(&mut self, cx: &App) -> gpui::FocusHandle {
        self.overflow_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Return focus to the overflow trigger, if it was ever shown.
    fn refocus_overflow(&self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = &self.overflow_focus {
            focus.focus(window, cx);
        }
    }

    fn overflow_open(&self) -> bool {
        self.overflow_open
    }

    fn toggle_overflow(&mut self) {
        self.overflow_open = !self.overflow_open;
    }

    fn close_overflow(&mut self) {
        self.overflow_open = false;
    }

    /// Follow the pointer over a tab. Returns whether the popped tab changed.
    /// Leaving a tab clears the hover only while it still owns it.
    fn hover(&mut self, key: u64, entered: bool) -> bool {
        let next = if entered {
            Some(key)
        } else if self.hovered == Some(key) {
            None
        } else {
            return false;
        };
        let changed = self.hovered != next;
        self.hovered = next;
        changed
    }

    /// Where the popped-out tab sits among `keys`, if it is still shown.
    fn hovered_position(&self, keys: &[u64]) -> Option<usize> {
        self.hovered
            .and_then(|key| keys.iter().position(|k| *k == key))
    }

    /// Start recording where the tab header paints this frame.
    fn begin_header(&mut self, track_left: f32, version_width: f32) {
        self.header_offset = track_left;
        self.version_width = version_width;
        self.hit_targets.clear();
    }

    fn record_hit_target(&mut self, index: usize, x: f32) {
        self.hit_targets.push((index, x));
    }

    /// Where each visible tab's center sits, relative to the header track.
    pub(super) fn hit_targets(&self) -> &[(usize, f32)] {
        &self.hit_targets
    }

    pub(super) fn header_offset(&self) -> f32 {
        self.header_offset
    }

    /// Re-express the header track in canvas coordinates.
    pub(super) fn shift_header(&mut self, by: f32) {
        self.header_offset += by;
    }

    pub(super) fn version_width(&self) -> f32 {
        self.version_width
    }

    pub(super) fn is_animating(&self) -> bool {
        self.tabs.values().any(|tab| {
            tab.left.is_animating() || tab.width.is_animating() || tab.height.is_animating()
        })
    }

    #[cfg(test)]
    fn settle(&mut self) {
        for tab in self.tabs.values_mut() {
            tab.sample(Instant::now() + Duration::from_secs(1));
        }
    }
}

/// Hover text for a session tab or tab-strip control.
pub(super) fn tab_tooltip(text: impl Into<gpui::SharedString>) -> Tooltip {
    Tooltip::text(text)
        .max_width(px(360.0))
        .debug_selector("live-session-tab-tooltip")
}

impl Workspace {
    /// Workspace a pointer "next" click lands on: the next occupied workspace
    /// below (wrapping), or a fresh adjacent one when nothing else is open.
    pub(super) fn next_workspace_row(&self) -> usize {
        let occupied = |row: usize| {
            self.row_indices(row)
                .any(|index| !self.slots[index].closing)
        };
        (1..STRIP_COUNT)
            .map(|step| (self.active_row + step) % STRIP_COUNT)
            .find(|&row| occupied(row))
            .unwrap_or((self.active_row + 1) % STRIP_COUNT)
    }

    fn tab_action(id: impl Into<gpui::ElementId>, size: f32) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex_none()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
    }

    fn render_tab_rename(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        Self::tab_action("rename-session-button", TAB_RENAME_SIZE)
            .debug_selector(|| "rename-session-button".into())
            .hover(|style| style.bg(Theme::global().PANEL_BG))
            .tooltip(tab_tooltip("Rename session").key_hint("F2").builder())
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.rename_session(&RenameSession, window, cx);
            }))
            .child(
                gpui::svg()
                    .data(include_bytes!("../../../assets/icons/pencil.svg"))
                    .size(px(12.0))
                    .text_color(Theme::global().TEXT_DIM),
            )
    }

    fn render_tab_close(
        &self,
        index: usize,
        diameter: f32,
        colors: TabCloseColors,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        crate::keyboard::tracked_button(
            Self::tab_action(("close-session-button", index), diameter),
            focus,
        )
        .debug_selector(move || format!("close-session-button-{index}"))
        .group("tab-close-hitbox")
        .tooltip(
            tab_tooltip("Close tab")
                .key_hint(if cfg!(target_os = "macos") {
                    "⌘Q"
                } else {
                    "Super+Q"
                })
                .builder(),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.set_active(index, cx);
            this.close_panel(&ClosePanel, window, cx);
        }))
        .child(
            div()
                .id(("close-session-pill", index))
                .debug_selector(move || format!("close-session-pill-{index}"))
                .flex_none()
                .size(px(TAB_CLOSE_PILL_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(colors.background)
                .text_color(colors.glyph)
                .text_size(px(TAB_CLOSE_GLYPH_SIZE))
                .group_hover("tab-close-hitbox", |style| {
                    style
                        .bg(Theme::global().ERROR_BG)
                        .text_color(Theme::global().ERROR)
                })
                .child("×"),
        )
    }

    fn render_next_workspace_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let target = self.next_workspace_row();
        let accent = Theme::global().workspace_accent(target);
        let shortcut = if cfg!(target_os = "macos") {
            "⌘J / ⌘K"
        } else {
            "Super+J / Super+K"
        };
        let tooltip: gpui::SharedString =
            format!("Go to workspace {} ({shortcut})", target + 1).into();
        tab_action_button("tab-next-workspace")
            .gap(px(2.0))
            .bg(accent.opacity(0.12))
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(accent)
            .hover(move |el| el.bg(accent.opacity(0.26)))
            .tooltip(tab_tooltip(tooltip.clone()).builder())
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    // Pointer route: coach the Super+J/K shortcut.
                    this.missed("focus_up_down", cx);
                    let row = this.next_workspace_row();
                    this.overview = false;
                    this.overview_progress.set(0.0, Instant::now());
                    this.switch_row_animated(row, window, cx);
                    cx.notify();
                }),
            )
            .child(div().text_size(px(10.0)).child("↓"))
            .child((target + 1).to_string())
    }
}

impl Workspace {
    fn render_tab_overflow(
        &mut self,
        hidden: &[usize],
        right: f32,
        row_width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let focus = self.live_tabs.overflow_focus(cx);
        let open = self.live_tabs.overflow_open();
        let theme = Theme::global();
        let root = div()
            .id("tab-overflow-popover")
            .absolute()
            .top_0()
            .right(px(right))
            .w(px((row_width - right).clamp(TAB_OVERFLOW_WIDTH, 240.0)))
            .flex()
            .flex_col()
            .items_end();
        let trigger = crate::keyboard::popover_trigger(
            div()
                .id("tab-overflow")
                .debug_selector(|| "tab-overflow".into()),
            &focus,
        )
        .w(px(TAB_OVERFLOW_WIDTH))
        .h(px(TAB_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(px(11.0))
        .text_color(theme.TEXT_DIM)
        .hover(|el| {
            el.bg(Theme::global().PANEL_BG)
                .text_color(Theme::global().TEXT)
        })
        .cursor_pointer()
        .occlude()
        .tooltip(tab_tooltip("Hidden session tabs").builder())
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.live_tabs.toggle_overflow();
            focus.focus(window, cx);
            cx.notify();
        }))
        .child(
            div()
                .min_w_0()
                .max_w_full()
                .truncate()
                .child(format!("↓ {}", hidden.len())),
        );
        root.child(trigger)
            .when(open, |root| {
                root.on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.live_tabs.close_overflow();
                    cx.notify();
                }))
                .key_context(crate::menu::CONTEXT)
                .on_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                    this.live_tabs.close_overflow();
                    this.live_tabs.refocus_overflow(window, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .id("tab-overflow-menu")
                        .debug_selector(|| "tab-overflow-menu".into())
                        .mt_1()
                        .w_full()
                        .max_h(px(240.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_1()
                        .rounded_lg()
                        .shadow_md()
                        .bg(theme.PANEL_BG)
                        .border_1()
                        .border_color(theme.PANEL_BORDER)
                        .occlude()
                        .children(hidden.iter().map(|&index| {
                            let panel = self.panel_metadata(&self.slots[index], cx);
                            let title =
                                folder_session_title(&panel.session_id, panel.title.as_ref());
                            let entity = cx.weak_entity();
                            crate::keyboard::button(div().id(("tab-overflow-item", index)))
                                .debug_selector(move || format!("tab-overflow-item-{index}"))
                                .flex_none()
                                .rounded_full()
                                .cursor_pointer()
                                .px_2()
                                .py_1()
                                .text_size(px(12.0))
                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                .child(div().truncate().child(title))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let _ = entity.update(cx, |this, cx| {
                                        this.live_tabs.close_overflow();
                                        this.activate_live_tab(index, window, cx);
                                    });
                                })
                        })),
                )
            })
            .into_any_element()
    }

    fn activate_live_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active(index, cx);
        self.overview = false;
        self.overview_progress.set(0.0, Instant::now());
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn render_workspace_bar(
        &mut self,
        canvas_width: f32,
        menu_button: bool,
        coach_progress: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let macos = cfg!(target_os = "macos");
        let windowed_macos = macos && !workspace_fullscreen(window);
        let right = if !macos
            && self.show_minimap
            && minimap_fits_header(canvas_width)
            && !self.slots.is_empty()
            && !self.overview
        {
            MINIMAP_WIDTH + MINIMAP_RIGHT + 8.0
        } else {
            0.0
        };
        let left = if windowed_macos {
            MACOS_TRAFFIC_LIGHTS_WIDTH
        } else {
            0.0
        };
        let show_fps = fps_enabled() && !menu_button;
        let track_left = left
            + if menu_button {
                TAB_MENU_WIDTH
            } else if show_fps {
                TAB_STATUS_WIDTH
            } else {
                0.0
            };
        let show_files_toggle = !self.files_pane.open;
        let controls_width = TAB_ACTIONS_WIDTH
            + if show_files_toggle {
                TAB_ACTION_SLOT
            } else {
                0.0
            };
        let drag_width = if windowed_macos {
            TAB_WINDOW_DRAG_WIDTH
        } else {
            0.0
        };
        let mut available =
            (canvas_width - right - track_left - controls_width - drag_width).max(0.0);
        let can_rename = self.slots.get(self.active).is_some_and(|slot| {
            slot.row == self.active_row && !slot.closing && self.panel_metadata(slot, cx).can_rename
        });
        let mut entries = Vec::new();
        for row in 0..STRIP_COUNT {
            // Closing surfaces stay mounted for their fade, but their tabs
            // must leave now. Otherwise selection first animates right in the
            // old group, then reverses when the dismissed slot is retired.
            let indices: Vec<_> = self
                .row_indices(row)
                .filter(|&index| !self.slots[index].closing)
                .collect();
            if indices.is_empty() && row == self.active_row {
                entries.push((None, row, 0));
            }
            for (row_position, index) in indices.into_iter().enumerate() {
                entries.push((Some(index), row, row_position));
            }
        }
        let selected_entry = |entries: &[TabEntry]| {
            entries
                .iter()
                .position(|(index, row, _)| {
                    *row == self.active_row && index.is_none_or(|index| index == self.active)
                })
                .unwrap_or(0)
        };
        let selected = selected_entry(&entries);
        let hidden = fit_tabs(&mut entries, selected, available);
        let selected = selected_entry(&entries);
        let overflow_width = if hidden.is_empty() {
            0.0
        } else {
            TAB_OVERFLOW_WIDTH
        };
        available = (available - overflow_width).max(0.0);
        if hidden.is_empty() {
            self.live_tabs.close_overflow();
        }
        let rows: Vec<_> = entries.iter().map(|(_, row, _)| *row).collect();
        let layout = TabLayout::grouped(available, &rows, selected);
        let keys: Vec<u64> = entries
            .iter()
            .map(|(index, row, _)| {
                index
                    .map(|index| self.slots[index].panel.entity_id().as_u64())
                    .unwrap_or(u64::MAX - *row as u64)
            })
            .collect();
        // The hovered tab pops out over its neighbours. Layout (hit targets,
        // coach space) still uses the resting geometry.
        let hovered = self
            .live_tabs
            .hovered_position(&keys)
            .filter(|&position| entries[position].0.is_some());
        let focused_width = layout.get(selected).map_or(0.0, |tab| tab.width);
        let targets: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(position, key)| {
                let tab = layout[position];
                let tab = if Some(position) == hovered {
                    let mut tab = if position == selected {
                        tab
                    } else {
                        TabLayout::pop_out(tab, focused_width, available)
                    };
                    tab.height += TAB_HOVER_LIFT;
                    tab
                } else {
                    tab
                };
                (*key, tab)
            })
            .collect();
        // Never chase camera coordinates or clipped-panel widths. Focus only
        // nudges tab geometry, with its own short, continuously retargeted tween.
        let geometry = self.live_tabs.sample(
            &targets,
            available,
            transition::policy(Transition::Focus).duration,
            Instant::now(),
        );
        // Never reserve width or squeeze the tabs for a hint. Check both the
        // current and destination geometry so an incoming tab cannot cross it.
        let coach_left = coach_chip_left(available, &geometry, &layout);
        if coach_left.is_none() {
            self.coach.hover_hint(false, learning::now());
        }
        let coach_chip = coach_left.and_then(|left| {
            self.coach_display_hint
                .as_ref()
                .filter(|_| coach_progress > 0.0)
                .map(|hint| self.render_coach_chip(hint, coach_progress, track_left + left, cx))
        });
        let mut tabs = div()
            .id("live-session-tabs")
            .debug_selector(|| "live-session-tabs".into())
            .absolute()
            .top_0()
            .left(px(track_left))
            .right(px(controls_width + overflow_width + drag_width))
            .h(px(FOLDER_CONTENT_INSET));
        self.live_tabs
            .begin_header(track_left, version_header_width(available));
        let paint_order = TabLayout::hover_paint_order(entries.len(), selected, hovered);
        let mut tab_bodies = Vec::with_capacity(entries.len());
        let mut slots = Vec::with_capacity(entries.len());
        for &position in &paint_order {
            let (index, row, _) = entries[position];
            let focused = position == selected;
            let hot = Some(position) == hovered;
            let popped = hot && !focused;
            let compact = row != self.active_row && !popped;
            let key = keys[position];
            let accent = Theme::global().workspace_accent(row);
            let background = Theme::global()
                .panel_background(focused)
                .blend(accent.opacity(if focused { 0.16 } else { 0.05 }));
            let current = geometry[position];
            let (exposed_left, visible) = TabLayout::exposed_in(&geometry, position, &paint_order);
            // The pointer target is the tab's resting slot, except while it is
            // popped: then it is the whole popped body (its final geometry,
            // not the animating one), so the actions at the body's right edge
            // stay reachable and what you see is what you click. The popped
            // body always covers the resting slot, so it cannot flicker.
            let (rest_left, rest_width) = TabLayout::exposed(&layout, position, selected);
            let (slot_left, slot_width) = if hot {
                (targets[position].1.left, targets[position].1.width)
            } else {
                (rest_left, rest_width)
            };
            if let Some(index) = index {
                let x = rest_left + rest_width / 2.0;
                self.live_tabs.record_hit_target(index, x);
            }
            let padding = (visible / 12.0).min(6.0);
            let (title, emoji, ring, outline) = match index {
                Some(index) => {
                    let panel = self.panel_metadata(&self.slots[index], cx);
                    (
                        folder_session_title(&panel.session_id, panel.title.as_ref()),
                        jcode_core::id::extract_session_name(&panel.session_id)
                            .map(jcode_core::id::session_icon)
                            .unwrap_or("💫"),
                        panel.tab_ring,
                        panel.tab_outline.clone(),
                    )
                }
                None => ("Empty workspace".into(), "📁", None, None),
            };
            let base = if hot {
                background.blend(accent.opacity(0.12))
            } else {
                background
            };
            let fill = tab_surface(ring.map_or(base, |ring| base.blend(ring.tint())));
            let close_colors = TabCloseColors::for_tab(ring, accent);
            let right = current.left + current.width;
            let diameter = current.height;
            let center_offset = if macos {
                ((TAB_HEIGHT - diameter) / 2.0).max(0.0)
            } else {
                0.0
            };
            // A stacked tab cannot draw a close cap on top of its neighbour.
            let close_index = index.filter(|_| {
                !compact && visible >= diameter + 24.0 && exposed_left + visible >= right - 0.01
            });
            let close_focus = close_index.map(|_| self.live_tabs.close_focus(key, cx));
            let show_close = close_focus.as_ref().is_some_and(|handle| {
                hot || (window.last_input_was_keyboard() && handle.is_focused(window))
            });
            let show_rename = show_close && hot && focused && can_rename && visible >= 88.0;
            let actions_width = if show_close { diameter } else { 0.0 }
                + if show_rename { TAB_RENAME_SIZE } else { 0.0 };
            let border_width = (current.width / 2.0).min(1.0);
            let title_inset = 2.0;
            // Paint the title at the same width, but leave no visible text
            // inside the action hitboxes when they are revealed.
            let title_cover = (actions_width + border_width - title_inset - padding).max(0.0);
            let fade_width = if show_close { 16.0 } else { 20.0 };
            let fade = gpui::linear_gradient(
                90.0,
                gpui::linear_color_stop(fill.opacity(0.0), 0.0),
                gpui::linear_color_stop(fill, 1.0),
            );
            let text = div()
                .absolute()
                .left(px(exposed_left - current.left))
                .flex_none()
                .w(px((visible - title_inset).max(0.0)))
                .min_w_0()
                .overflow_hidden()
                .px(px(padding))
                .h_full()
                .flex()
                .items_center()
                .gap(px((visible / 12.0).min(4.0)))
                .child(
                    div()
                        .debug_selector(move || match index {
                            Some(index) => format!("live-session-tab-{index}-emoji"),
                            None => "live-session-empty-tab-emoji".into(),
                        })
                        .flex_none()
                        .size(px((visible - 2.0 - 2.0 * padding).clamp(1.0, 20.0)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px((visible - 2.0 - 2.0 * padding).clamp(1.0, 14.0)))
                        .child(emoji),
                )
                .when(!compact && (focused || visible >= 52.0), |el| {
                    el.child(
                        div()
                            .debug_selector(move || match index {
                                Some(index) => format!("live-session-tab-{index}-title"),
                                None => "live-session-empty-tab-title".into(),
                            })
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(title.clone())
                            .child(
                                div()
                                    .debug_selector(move || match index {
                                        Some(index) => format!("live-session-tab-{index}-fade"),
                                        None => "live-session-empty-tab-fade".into(),
                                    })
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .right(px(title_cover))
                                    .w(px(fade_width))
                                    .bg(fade),
                            )
                            .when(show_close, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .right_0()
                                        .w(px(title_cover))
                                        .bg(fill),
                                )
                            }),
                    )
                });
            tab_bodies.push(
                div()
                    .debug_selector(move || match index {
                        Some(index) => format!("live-session-tab-{index}"),
                        None => "live-session-empty-tab".into(),
                    })
                    .absolute()
                    .left(px(current.left))
                    .bottom(px(TAB_FLOAT_GAP + center_offset))
                    .w(px(current.width))
                    .min_w_0()
                    .overflow_hidden()
                    .h(px(current.height))
                    .flex()
                    .items_center()
                    .rounded(crate::panel::tab_corner_radius(gpui::size(
                        px(current.width),
                        px(current.height),
                    )))
                    // Crowded off-screen tabs can be narrower than two pixels.
                    // Their border must not force the layout wider than its slot.
                    // Use this single outline on every edge. An extra accent
                    // stripe makes the top heavier and squares off the corners.
                    .border(px(border_width))
                    .border_color(match ring {
                        // The ring paints the outline itself. Hide the static
                        // border so progress reads as filling, not overlaying.
                        Some(_) => accent.opacity(0.0),
                        None if focused => accent,
                        None => accent.opacity(0.35),
                    })
                    .bg(fill)
                    .text_size(px(11.0))
                    .when(focused, |el| el.font_weight(gpui::FontWeight::SEMIBOLD))
                    .when(popped, |el| el.shadow_md())
                    .text_color(if focused || hot {
                        Theme::global().TEXT
                    } else {
                        Theme::global().TEXT_DIM
                    })
                    .when(focused && index.is_some(), |el| {
                        el.child(div().absolute().inset_0().debug_selector(move || {
                            format!("live-session-tab-{}-focused", index.unwrap())
                        }))
                    })
                    .child(text)
                    .when_some(outline.filter(|_| ring.is_some()), |el, outline| {
                        let index = index.unwrap();
                        el.child(
                            div()
                                .debug_selector(move || format!("live-session-tab-{index}-ring"))
                                .absolute()
                                .inset_0()
                                .child(outline),
                        )
                    }),
            );
            slots.push(
                div()
                    .id(("workspace-session", index.unwrap_or(usize::MAX)))
                    .debug_selector(move || match index {
                        Some(index) => format!("live-session-tab-{index}-slot"),
                        None => "live-session-empty-tab-slot".into(),
                    })
                    .absolute()
                    .left(px(slot_left))
                    .bottom(px(TAB_FLOAT_GAP))
                    .w(px(slot_width))
                    .h(px(if hot {
                        targets[position].1.height
                    } else {
                        TAB_HEIGHT
                    }))
                    .occlude()
                    .cursor_pointer()
                    // The hit layer keeps actions reachable while the tab pops
                    // over its neighbours. Use the painted body's current edge
                    // and height, not the settled (pointer target) geometry.
                    .when_some(close_index.zip(close_focus), |el, (index, focus)| {
                        if !show_close {
                            // Keep the Tab stop without reserving title width or
                            // leaving an invisible pointer target over the title.
                            return el.child(
                                crate::keyboard::tracked_button(
                                    div().id(("close-session-button", index)),
                                    &focus,
                                )
                                .absolute()
                                .size_0(),
                            );
                        }
                        el.child(
                            div()
                                .absolute()
                                .bottom(px(center_offset))
                                .left(px(right - slot_left - actions_width))
                                .w(px(actions_width))
                                .h(px(diameter))
                                .flex()
                                .items_center()
                                .justify_end()
                                .when(show_rename, |el| el.child(self.render_tab_rename(cx)))
                                .child(self.render_tab_close(
                                    index,
                                    diameter,
                                    close_colors,
                                    &focus,
                                    cx,
                                )),
                        )
                    })
                    .when(index.is_some(), |el| {
                        el.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if this.live_tabs.hover(key, *hovered) {
                                cx.notify();
                            }
                        }))
                    })
                    .tooltip(tab_tooltip(format!("Workspace {} · {title}", row + 1)).builder())
                    .when_some(index, |el, index| {
                        let entity = cx.weak_entity();
                        crate::keyboard::press_button(el, move |window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            let _ = entity.update(cx, |this, cx| {
                                this.activate_live_tab(index, window, cx);
                            });
                        })
                    }),
            );
        }
        // Workspace numbers belong behind the floating tab bodies. In
        // particular, a hovered tab may cover another workspace's label.
        // Keep interaction slots above both layers without repainting ink.
        let group_count = entries
            .iter()
            .filter(|(_, _, position)| *position == 0)
            .count();
        let label_width = group_label_width(available, group_count);
        for (position, &(_, row, row_position)) in entries.iter().enumerate() {
            if row_position != 0 {
                continue;
            }
            let accent = Theme::global().workspace_accent(row);
            let active = row == self.active_row;
            tabs = tabs.child(
                div()
                    .id(("workspace-tab-group-label", row))
                    .debug_selector(move || format!("workspace-tab-group-label-{row}"))
                    .absolute()
                    .left(px((layout[position].left - label_width).max(0.0)))
                    .bottom(px(TAB_FLOAT_GAP))
                    .w(px(label_width))
                    .h(px(TAB_HEIGHT))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(accent)
                    .occlude()
                    .cursor_pointer()
                    .tooltip(tab_tooltip(format!("Workspace {}", row + 1)).builder())
                    .child(
                        div()
                            .w(px((label_width - 4.0).max(0.0)))
                            .rounded(px(3.0))
                            .text_center()
                            .when(active, |el| el.bg(accent.opacity(0.16)))
                            .child(format!("{}", row + 1)),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            let position = this.active_position_in_row();
                            this.select_row(row, position);
                            this.overview = false;
                            this.overview_progress.set(0.0, Instant::now());
                            this.focus_active(window, cx);
                            cx.notify();
                        }),
                    ),
            );
        }
        tabs = tabs.children(tab_bodies).children(slots);
        // Even when enabled, sample only on existing redraws.
        let fps = show_fps.then(|| {
            self.fps_counter
                .label(Instant::now(), || window.frame_duration_snapshot())
        });
        let overflow = (!hidden.is_empty())
            .then(|| self.render_tab_overflow(&hidden, controls_width, canvas_width - right, cx));
        div()
            .debug_selector(|| "workspace-tab-row".into())
            .absolute()
            .top(px(STRIP_PADDING_TOP))
            .left_0()
            .right(px(right))
            .h(px(FOLDER_CONTENT_INSET))
            .map(|el| {
                if menu_button {
                    el.child(self.render_compact_menu_button(left, cx))
                } else if let Some(fps) = fps {
                    el.child(
                        div()
                            .debug_selector(|| "fps-counter-slot".into())
                            .absolute()
                            .left(px(left))
                            .top_0()
                            .w(px(TAB_STATUS_WIDTH))
                            .h(px(TAB_HEIGHT))
                            .flex()
                            .items_center()
                            .pl_2()
                            .child(
                                div()
                                    .debug_selector(|| "fps-counter".into())
                                    .font_family(Theme::global().FONT_MONO)
                                    .text_size(px(10.0))
                                    .text_color(Theme::global().TEXT_DIM)
                                    .child(fps),
                            ),
                    )
                } else {
                    el
                }
            })
            .child(tabs)
            .children(coach_chip)
            .children(overflow)
            .child(self.render_tab_actions(show_files_toggle, cx))
            .into_any_element()
    }

    fn render_tab_actions(&self, show_files_toggle: bool, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .top_0()
            .right(px(TAB_CAPTION_WIDTH))
            .flex()
            .items_center()
            .gap(px(TAB_ACTION_GAP))
            .when(show_files_toggle, |row| {
                row.child(
                    self.files_button(
                        "workspace-files-toggle",
                        "Show Files",
                        "▤",
                        |workspace, window, cx| {
                            workspace.toggle_files(&super::files::ToggleFiles, window, cx)
                        },
                        cx,
                    )
                    .size(px(TAB_HEIGHT)),
                )
            })
            .child(self.render_next_workspace_button(cx))
            .child(
                tab_action_button("tab-new-session")
                    .children(crate::fixture_control_targets::marker(
                        || "tab-new-session".into(),
                        || None,
                    ))
                    .text_size(px(20.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .hover(|el| {
                        el.bg(Theme::global().wash(crate::theme::Ink::Text, 0.10))
                            .text_color(Theme::global().TEXT)
                    })
                    .tooltip(
                        tab_tooltip("New session")
                            .key_hint(if cfg!(target_os = "macos") {
                                "⌘N"
                            } else {
                                "Super+N"
                            })
                            .builder(),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.missed("new_panel", cx);
                            this.open_new_session(cx);
                        }),
                    )
                    .child("+"),
            )
            .when(!crate::window_caption::enabled(), |el| {
                el.child(
                    tab_action_button("tab-close-window")
                        .text_size(px(20.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .hover(|el| {
                            el.bg(Theme::global().wash(crate::theme::Ink::Error, 0.16))
                                .text_color(Theme::global().ERROR)
                        })
                        .tooltip(tab_tooltip("Close window").builder())
                        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                        })
                        .on_click(|_, window, cx| {
                            cx.stop_propagation();
                            // Build the host's action rather than passing a UI-generation
                            // Rust type across the hot-reload boundary. The host snapshots
                            // the workspace before removing its native window.
                            if let Ok(action) =
                                cx.build_action("jcode_desktop_host::CloseWindow", None)
                            {
                                window.dispatch_action(action, cx);
                            }
                        })
                        .child("×"),
                )
            })
    }
}

#[cfg(test)]
#[path = "live_tab_actions_tests.rs"]
mod action_tests;

#[cfg(test)]
mod tests {
    use super::*;

    gpui::actions!(jcode_desktop_host, [CloseWindow]);

    #[gpui::test]
    fn next_workspace_pill_reaches_the_next_occupied_workspace_by_mouse(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.push_test_panel("first", cx);
            w.active_row = 2;
            w.push_test_panel("third", cx);
            w.active_row = 0;
            w.active = 0;
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [1440., 800., 480.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            vcx.run_until_parked();
            let next = vcx.debug_bounds("tab-next-workspace").unwrap();
            let plus = vcx.debug_bounds("tab-new-session").unwrap();
            let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
            assert!(tabs.right() <= next.left(), "width={width}");
            assert!(next.right() <= plus.left(), "width={width}");
        }
        // Skips empty workspace 2 and lands on workspace 3, then wraps back.
        for expected in [2, 0] {
            let next = vcx.debug_bounds("tab-next-workspace").unwrap();
            vcx.simulate_click(next.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), expected);
        }
    }

    #[gpui::test]
    #[cfg(not(target_os = "windows"))]
    fn tab_close_window_is_separate_from_new_session_and_dispatches_host_action(
        cx: &mut gpui::TestAppContext,
    ) {
        let requests = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update({
            let requests = requests.clone();
            move |cx| {
                cx.on_action(move |_: &CloseWindow, _| requests.set(requests.get() + 1));
            }
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.push_test_panel("Keep this session", cx);
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [1440., 800., 480.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            vcx.run_until_parked();
            let close = vcx.debug_bounds("tab-close-window").unwrap();
            let plus = vcx.debug_bounds("tab-new-session").unwrap();
            let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
            let files = vcx.debug_bounds("workspace-files-toggle").unwrap();
            let next = vcx.debug_bounds("tab-next-workspace").unwrap();
            let actions = [files, next, plus, close];
            assert!(tabs.right() <= files.left());
            for action in actions {
                assert_eq!(action.size, gpui::size(px(28.), px(28.)));
                assert_eq!(action.top(), plus.top());
            }
            let gaps = [
                next.left() - files.right(),
                plus.left() - next.right(),
                close.left() - plus.right(),
            ];
            assert_eq!(gaps, [px(3.); 3]);
            assert!(close.right() <= px(width));
            vcx.simulate_click(close.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.slots.len()), 1);
        }
        assert_eq!(requests.get(), 3);
    }

    #[test]
    fn coaching_chip_never_claims_current_or_incoming_tab_space() {
        let left = TabGeometry {
            left: 100.0,
            width: 200.0,
            height: 32.0,
        };
        let right = TabGeometry {
            left: 600.0,
            ..left
        };
        assert_eq!(coach_chip_left(1000.0, &[left], &[left]), Some(692.0));
        assert_eq!(coach_chip_left(1000.0, &[left], &[right]), None);
        assert_eq!(coach_chip_left(1000.0, &[right], &[left]), None);
        for available in [0.0, 240.0, 400.0, 600.0] {
            assert_eq!(coach_chip_left(available, &[left], &[left]), None);
        }
    }

    #[test]
    fn workspace_tab_groups_prioritize_current_row_and_keep_number_order() {
        let rows = [0, 0, 1, 1, 1, 3, 3];
        for selected in 0..rows.len() {
            let tabs = TabLayout::grouped(1400.0, &rows, selected);
            for (i, tab) in tabs.iter().enumerate() {
                assert_eq!(
                    tab.width,
                    if i == selected {
                        208.0
                    } else if rows[i] == rows[selected] {
                        196.0
                    } else {
                        56.0
                    }
                );
                assert_eq!(
                    tab.height,
                    TAB_HEIGHT - if i == selected { 0.0 } else { 4.0 }
                );
                if i > 0 && rows[i - 1] != rows[i] {
                    assert!(
                        (tab.left
                            - tabs[i - 1].left
                            - tabs[i - 1].width
                            - TAB_GROUP_GAP
                            - TAB_GROUP_LABEL_WIDTH)
                            .abs()
                            < 0.001
                    );
                }
            }
            assert_eq!(tabs[0].left, TAB_GROUP_LABEL_WIDTH);
            let last = tabs.last().unwrap();
            assert!(last.left + last.width <= 1400.001);
        }
        // A single group preserves the tab motion within its label reservation.
        let old = TabLayout::new(512.0 - TAB_GROUP_LABEL_WIDTH, 12);
        for selected in 0..12 {
            assert_eq!(
                TabLayout::grouped(512.0, &[2; 12], selected),
                (0..12)
                    .map(|i| {
                        let mut tab = old.geometry(i, selected);
                        tab.left += TAB_GROUP_LABEL_WIDTH - old.start;
                        tab
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn crowded_workspace_tabs_keep_the_original_overlap() {
        let tabs = TabLayout::grouped(892.0, &[0; 12], 0);
        assert_eq!(tabs[1].left + tabs[1].width - tabs[2].left, 136.0);
    }

    #[test]
    fn workspace_tab_groups_keep_crowded_sessions_exposed_and_resize_in_bounds() {
        assert!(TabLayout::grouped(0.0, &[], 0).is_empty());
        for counts in [
            [1, 1, 1, 1],
            [1, 200, 1, 1],
            [80, 2, 100, 40],
            [20, 30, 40, 50],
        ] {
            let rows: Vec<_> = counts
                .iter()
                .enumerate()
                .flat_map(|(row, count)| std::iter::repeat_n(row, *count))
                .collect();
            for available in [0.0, 40.0, 100.0, 352.0, 512.0, 800.0, 1600.0] {
                for selected in [0, counts[0], counts[0] + counts[1], rows.len() - 1] {
                    let tabs = TabLayout::grouped(available, &rows, selected);
                    for (i, tab) in tabs.iter().enumerate() {
                        assert!(tab.left.is_finite() && tab.width.is_finite());
                        assert!(
                            tab.left >= 0.0 && tab.left + tab.width <= available + 0.001,
                            "available={available}, selected={selected}, i={i}, tab={tab:?}"
                        );
                        if available > 0.0 {
                            assert!(
                                TabLayout::exposed(&tabs, i, selected).1 > 0.0,
                                "every session must retain a click target: available={available}, selected={selected}, i={i}"
                            );
                            if i > 0 && rows[i - 1] != rows[i] {
                                assert!(tab.left > tabs[i - 1].left + tabs[i - 1].width);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn workspace_tab_groups_animate_focus_and_snap_safely_on_resize() {
        let now = Instant::now();
        let duration = Duration::from_millis(150);
        let rows = [0, 0, 1, 1, 2, 2];
        let targets = |available, selected| {
            TabLayout::grouped(available, &rows, selected)
                .into_iter()
                .enumerate()
                .map(|(i, tab)| (i as u64, tab))
                .collect::<Vec<_>>()
        };
        let mut motion = TabMotion::default();
        let first = motion.sample(&targets(1200.0, 0), 1200.0, duration, now);
        assert_eq!(
            first,
            motion.sample(&targets(1200.0, 3), 1200.0, duration, now)
        );
        let middle = motion.sample(
            &targets(1200.0, 3),
            1200.0,
            duration,
            now + Duration::from_millis(50),
        );
        assert!(middle[3].width > first[3].width && middle[3].width < 208.0);
        for i in [2, 4] {
            assert!(
                (middle[i].left
                    - middle[i - 1].left
                    - middle[i - 1].width
                    - TAB_GROUP_GAP
                    - TAB_GROUP_LABEL_WIDTH)
                    .abs()
                    < 0.001
            );
        }
        assert_eq!(
            middle,
            motion.sample(
                &targets(1200.0, 5),
                1200.0,
                duration,
                now + Duration::from_millis(50)
            ),
            "retargeting must not jump"
        );
        let resized = motion.sample(
            &targets(120.0, 5),
            120.0,
            duration,
            now + Duration::from_millis(60),
        );
        assert!(!motion.is_animating());
        assert_eq!(resized, TabLayout::grouped(120.0, &rows, 5));
        assert!(
            resized
                .iter()
                .all(|tab| tab.left >= 0.0 && tab.left + tab.width <= 120.001)
        );
        let reduced = motion.sample(&targets(120.0, 0), 120.0, Duration::ZERO, now);
        assert_eq!(reduced, TabLayout::grouped(120.0, &rows, 0));
        assert!(!motion.is_animating());
    }

    #[gpui::test]
    fn workspace_tab_groups_preserve_click_keyboard_and_empty_row_navigation(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for i in 0..9 {
                w.push_test_panel(&format!("group-session-{i}"), cx);
                w.slots[i].row = i / 3;
            }
            w
        });
        let handle = vcx.update(|window, cx| {
            window.focus(&workspace.read(cx).focus_handle.clone(), cx);
            window.window_handle()
        });
        for width in [1400.0, 640.0, 1000.0] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.0)));
            vcx.run_until_parked();
            for selected in [4, 8, 0, 7, 3, 2, 6, 5, 1] {
                workspace.update(vcx, |w, cx| {
                    // Measure resting layout, not a tab popped out under the pointer.
                    w.live_tabs.hovered = None;
                    w.live_tabs.settle();
                    cx.notify();
                });
                vcx.run_until_parked();
                let track = vcx.debug_bounds("live-session-tabs").unwrap();
                for row in 0..3 {
                    let label = vcx
                        .debug_bounds(format!("workspace-tab-group-label-{row}").leak())
                        .unwrap();
                    let first = vcx
                        .debug_bounds(format!("live-session-tab-{}", row * 3).leak())
                        .unwrap();
                    assert!(label.left() >= track.left() - px(0.01));
                    assert!(label.right() <= first.left() + px(0.01));
                    if row > 0 {
                        let previous = vcx
                            .debug_bounds(format!("live-session-tab-{}", row * 3 - 1).leak())
                            .unwrap();
                        assert!(label.left() > previous.right());
                    }
                    for position in row * 3..row * 3 + 3 {
                        assert!(
                            vcx.debug_bounds(
                                format!("workspace-tab-badge-{position}-row-{row}").leak()
                            )
                            .is_none(),
                            "workspace numbers belong beside the group, never inside a session tab"
                        );
                    }
                }
                let targets = workspace.read_with(vcx, |w, _| w.live_tabs.hit_targets().to_vec());
                assert_eq!(targets.len(), 9);
                for &(index, _) in &targets {
                    let tab = vcx
                        .debug_bounds(format!("live-session-tab-{index}").leak())
                        .unwrap();
                    assert!(tab.left() >= track.left() - px(0.01));
                    assert!(tab.right() <= track.right() + px(0.01));
                }
                let x = targets
                    .iter()
                    .find(|(index, _)| *index == selected)
                    .unwrap()
                    .1;
                vcx.simulate_click(
                    gpui::point(track.left() + px(x), track.bottom() - px(12.0)),
                    gpui::Modifiers::default(),
                );
                vcx.run_until_parked();
                // Leave the tab row so the next pass measures resting geometry.
                vcx.simulate_mouse_move(
                    gpui::point(px(5.0), px(600.0)),
                    None,
                    gpui::Modifiers::default(),
                );
                vcx.run_until_parked();
                workspace.update_in(vcx, |w, window, cx| {
                    assert_eq!(w.active, selected, "width={width}");
                    assert_eq!(w.active_row, selected / 3);
                    assert_eq!(w.navigation_state(window, cx)["keyboard_panel"], selected);
                    assert!(!w.overview);
                });
            }
        }
        for row in [2, 1, 0] {
            workspace.update(vcx, |w, cx| {
                w.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
            let label = vcx
                .debug_bounds(format!("workspace-tab-group-label-{row}").leak())
                .unwrap();
            vcx.simulate_click(label.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), row);
        }
        // Workspace keys still select remembered sessions and the empty row
        // receives its own full-size placeholder rather than false focus.
        vcx.simulate_keystrokes("super-j super-j super-j");
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 3);
        assert!(vcx.debug_bounds("live-session-empty-tab-title").is_some());
        vcx.simulate_keystrokes("super-k");
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 2);
        assert!(vcx.debug_bounds("live-session-empty-tab").is_none());
        vcx.simulate_keystrokes("super-p");
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active), 8);
    }

    #[gpui::test]
    fn workspace_identity_follows_rows_and_number_badges_navigate(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.show_sidebar = false;
            workspace.enable_test_minimap();
            for row in 0..STRIP_COUNT {
                workspace.push_test_panel(&format!("identity-{row}"), cx);
                workspace.slots[row].row = row;
            }
            workspace
        });
        vcx.run_until_parked();
        for row in 0..STRIP_COUNT {
            assert!(
                vcx.debug_bounds(format!("workspace-tab-group-label-{row}").leak())
                    .is_some()
            );
            assert!(
                vcx.debug_bounds(format!("workspace-tab-accent-{row}-row-{row}").leak())
                    .is_none(),
                "workspace identity must not add a heavy stripe over the tab outline"
            );
        }
        for row in [1, 3, 2, 0] {
            let badge = vcx
                .debug_bounds(format!("workspace-map-badge-{row}").leak())
                .unwrap();
            let track = vcx
                .debug_bounds(format!("minimap-row-{row}").leak())
                .unwrap();
            assert!(
                badge.right() < track.left(),
                "numbers must not obscure the map"
            );
            vcx.simulate_click(badge.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(
                workspace.read_with(vcx, |workspace, _| workspace.active_row),
                row
            );
            assert!(
                vcx.debug_bounds(format!("workspace-map-badge-{row}-active").leak())
                    .is_some()
            );
        }
        workspace.update(vcx, |workspace, cx| {
            workspace.slots[0].row = 1;
            workspace.set_active(0, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("workspace-tab-group-label-1").is_some());
        assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_none());
        let empty = vcx.debug_bounds("workspace-map-badge-0").unwrap();
        vcx.simulate_click(empty.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("live-session-empty-tab-title").is_some());
        assert!(vcx.debug_bounds("workspace-map-badge-0-active").is_some());
        assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
        workspace.update(vcx, |workspace, cx| {
            workspace.show_minimap = false;
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
        assert!(vcx.debug_bounds("minimap").is_none());
    }

    #[gpui::test]
    fn live_tabs_ring_the_outline_while_working_and_keep_the_emoji_still(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.show_sidebar = false;
            workspace.push_test_panel("activity-tab", cx);
            workspace.push_test_panel("quiet-tab", cx);
            workspace
        });
        vcx.run_until_parked();
        let idle_title_left = vcx.debug_bounds("live-session-tab-0-title").unwrap().left();
        for (status, active) in [
            ("running", true),
            ("thinking", true),
            ("generating", true),
            ("streaming", true),
            ("running_tools", true),
            ("busy", true),
            ("idle", false),
            ("attached", false),
            ("connected", false),
            ("lost: disconnected", false),
            ("error", false),
            ("crashed", false),
        ] {
            workspace.update(vcx, |workspace, cx| {
                workspace.apply(
                    Update::Event {
                        session_id: "activity-tab".into(),
                        event: jcode_sdk::ApiEvent::SessionStatus {
                            session_id: "activity-tab".into(),
                            status: status.into(),
                            pending_soft_interrupts: None,
                        },
                    },
                    cx,
                );
                cx.notify();
            });
            vcx.run_until_parked();
            // Live turns paint the outline ring. Quiet states never animate,
            // and the emoji itself never animates in any state.
            let quiet = matches!(status, "idle" | "attached" | "connected");
            if active || quiet {
                assert_eq!(
                    vcx.debug_bounds("live-session-tab-0-ring").is_some(),
                    active,
                    "status {status}",
                );
            }
            assert!(vcx.debug_bounds("live-session-tab-1-spinner").is_none());
            assert!(vcx.debug_bounds("panel-session-title").is_none());
            // Active sessions also show an inline transcript status.
            assert_eq!(
                vcx.debug_bounds("panel-activity-label").is_some(),
                active,
                "inline status {status}",
            );
            let title = vcx.debug_bounds("live-session-tab-0-title").unwrap();
            assert!(vcx.debug_bounds("live-session-tab-0-spinner").is_none());
            assert!(vcx.debug_bounds("live-session-tab-1-ring").is_none());
            let emoji = vcx.debug_bounds("live-session-tab-0-emoji").unwrap();
            assert!(emoji.right() <= title.left());
            assert_eq!(
                title.left(),
                idle_title_left,
                "activity must not shift the title"
            );
        }
    }

    #[test]
    fn live_tabs_leave_a_small_gap_between_uncrowded_sessions() {
        for rows in [vec![0, 0, 0, 0], vec![0, 0, 1, 1]] {
            for selected in 0..rows.len() {
                let tabs = TabLayout::grouped(1600.0, &rows, selected);
                for (i, pair) in tabs.windows(2).enumerate() {
                    if rows[i] == rows[i + 1] {
                        assert!(
                            (pair[1].left - pair[0].left - pair[0].width - TAB_GAP).abs() < 0.001
                        );
                    }
                }
            }
        }
    }

    /// A popped tab owns its whole popped body: hover stays on it while the
    /// pointer travels to its close button at the body's right edge, and a
    /// click there closes that tab. Its resting slot never moves.
    #[gpui::test]
    fn popped_tab_owns_its_body_and_its_close_sits_at_the_body_edge(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for i in 0..8 {
                w.push_test_panel(&format!("hover-session-{i}"), cx);
            }
            w
        });
        let handle = vcx.update(|window, cx| {
            window.focus(&workspace.read(cx).focus_handle.clone(), cx);
            window.window_handle()
        });
        vcx.simulate_window_resize(handle, gpui::size(px(700.0), px(700.0)));
        workspace.update(vcx, |w, cx| {
            w.set_active(3, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        let rest: Vec<_> = (0..8)
            .map(|i| {
                vcx.debug_bounds(format!("live-session-tab-{i}-slot").leak())
                    .unwrap()
                    .center()
            })
            .collect();
        let away = gpui::point(px(350.0), px(400.0));
        let settle = |vcx: &mut gpui::VisualTestContext| {
            vcx.run_until_parked();
            workspace.update(vcx, |w, cx| {
                w.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
        };
        for i in [0, 1, 2, 4, 5, 6, 7] {
            vcx.simulate_mouse_move(away, None, gpui::Modifiers::default());
            settle(vcx);
            vcx.simulate_mouse_move(rest[i], None, gpui::Modifiers::default());
            settle(vcx);
            workspace.read_with(vcx, |w, _| {
                let key = w.slots[i].panel.entity_id().as_u64();
                assert_eq!(w.live_tabs.hovered, Some(key), "pointer over slot {i}");
                assert_eq!(w.active, 3, "hover never changes focus");
            });
            let body = vcx
                .debug_bounds(format!("live-session-tab-{i}").leak())
                .unwrap();
            let slot = vcx
                .debug_bounds(format!("live-session-tab-{i}-slot").leak())
                .unwrap();
            assert!(
                slot.contains(&rest[i]),
                "popped body still holds the pointer"
            );
            assert_eq!(slot, body, "the hit area is exactly the popped body");
            let close = vcx
                .debug_bounds(format!("close-session-button-{i}").leak())
                .unwrap();
            action_tests::assert_close_fits_cap(body, close);
            // Travel to the X: hover stays on this tab the whole way.
            for step in 1..=4 {
                let t = step as f32 / 4.0;
                let x = rest[i].x + (close.center().x - rest[i].x) * t;
                vcx.simulate_mouse_move(
                    gpui::point(x, close.center().y),
                    None,
                    gpui::Modifiers::default(),
                );
                settle(vcx);
                workspace.read_with(vcx, |w, _| {
                    let key = w.slots[i].panel.entity_id().as_u64();
                    assert_eq!(w.live_tabs.hovered, Some(key), "tab {i} stays popped");
                });
            }
        }
        // Clicking the X at the popped edge closes that tab, not a neighbour.
        vcx.simulate_mouse_move(away, None, gpui::Modifiers::default());
        settle(vcx);
        vcx.simulate_mouse_move(rest[5], None, gpui::Modifiers::default());
        settle(vcx);
        let close = vcx.debug_bounds("close-session-button-5").unwrap();
        vcx.simulate_mouse_move(close.center(), None, gpui::Modifiers::default());
        settle(vcx);
        vcx.simulate_click(close.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            let closed: Vec<_> = w
                .slots
                .iter()
                .filter(|slot| slot.closing)
                .map(|slot| slot.panel.read(cx).session_id.clone())
                .collect();
            assert_eq!(closed, ["hover-session-5"]);
        });
    }

    #[gpui::test]
    fn workspace_number_stays_in_its_resting_slot_when_tab_pops_out(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for row in 0..3 {
                w.active_row = row;
                w.push_test_panel(&format!("workspace-{row}"), cx);
            }
            w.active_row = 0;
            w.active = 0;
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(480.0), px(600.0)));
        vcx.run_until_parked();
        let before = vcx.debug_bounds("workspace-tab-group-label-2").unwrap();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.hovered = Some(w.slots[2].panel.entity_id().as_u64());
            cx.notify();
        });
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        assert_eq!(
            vcx.debug_bounds("workspace-tab-group-label-2").unwrap(),
            before
        );
        let tab = vcx.debug_bounds("live-session-tab-2").unwrap();
        assert!(
            tab.left() < before.right(),
            "pop-out must cover the resting badge"
        );
    }

    #[test]
    fn stacked_tab_surfaces_occlude_underlying_titles_in_translucent_themes() {
        for alpha in [0.0, 0.35, 0.75, 1.0] {
            let color = gpui::Rgba {
                r: 0.12,
                g: 0.18,
                b: 0.24,
                a: alpha,
            };
            let surface = tab_surface(color);
            assert_eq!(surface.a, 1.0);
            assert_eq!(
                (surface.r, surface.g, surface.b),
                (color.r, color.g, color.b)
            );
        }
    }

    #[test]
    fn hovered_tab_pops_out_on_top_without_leaving_the_pointer() {
        let order = TabLayout::hover_paint_order(4, 1, Some(3));
        assert_eq!(order.last(), Some(&3), "hovered tab paints above focus");
        assert_eq!(order.len(), 4);
        assert_eq!(TabLayout::hover_paint_order(4, 1, None).last(), Some(&1));
        for available in [100.0, 400.0, 1200.0] {
            for left in [0.0, 50.0, available - 40.0] {
                let rest = TabGeometry {
                    left,
                    width: 40.0,
                    height: TAB_HEIGHT - 4.0,
                };
                let pop = TabLayout::pop_out(rest, 208.0, available);
                assert_eq!(pop.height, TAB_HEIGHT);
                assert!(pop.width >= rest.width);
                assert!(pop.left >= 0.0 && pop.left + pop.width <= available + 0.001);
                // Every point of the resting tab stays covered by the popped tab.
                assert!(pop.left <= rest.left + 0.001);
                assert!(pop.left + pop.width >= rest.left + rest.width - 0.001);
            }
        }
    }

    #[test]
    fn live_tabs_share_crowded_width_and_keep_the_selection_largest() {
        for available in [0.0, 40.0, 180.0, 352.0, 800.0, 2400.0] {
            for count in 1..=200 {
                let layout = TabLayout::new(available, count);
                for selected in [0, count / 2, count - 1] {
                    let first = layout.geometry(0, selected);
                    let last = layout.geometry(count - 1, selected);
                    assert!(first.left >= 0.0);
                    assert!(last.left + last.width <= available + 0.001);
                    assert!((first.left - (available - last.left - last.width)).abs() < 0.001);
                    let active = layout.geometry(selected, selected);
                    assert!(active.width >= layout.inactive_width);
                    assert_eq!(
                        TabLayout::paint_order(count, selected).last(),
                        Some(selected)
                    );
                    if available > 0.0 && count > 1 {
                        assert!(layout.step > 0.0);
                    }
                }
            }
        }
        let crowded = TabLayout::new(512.0, 12);
        assert!(crowded.active_width - crowded.inactive_width <= 12.0);
        assert!(crowded.step > 27.0, "inactive tab icons remain exposed");
        assert!(TabLayout::new(1152.0, 3).start > 0.0);
    }

    #[test]
    fn live_tabs_center_two_and_four_panel_groups_with_subtle_focus_motion() {
        for available in [40.0, 192.0, 512.0, 1152.0, 1600.0] {
            for count in [2, 4, 6, 12, 200] {
                let layout = TabLayout::new(available, count);
                for selected in 0..count {
                    let tabs: Vec<_> = (0..count).map(|i| layout.geometry(i, selected)).collect();
                    let first = tabs[0];
                    let last = tabs[count - 1];
                    assert!((first.left + last.left + last.width - available).abs() < 0.001);
                    for (i, tab) in tabs.iter().enumerate() {
                        assert!(tab.left >= 0.0);
                        assert!(tab.left + tab.width <= available + 0.001);
                        assert!(TabLayout::exposed(&tabs, i, selected).1 > 0.0);
                        let next = layout.geometry(i, (selected + 1) % count);
                        assert!((next.left - tab.left).abs() <= 12.001);
                        assert!((next.width - tab.width).abs() <= 12.001);
                        assert!((next.height - tab.height).abs() <= 4.001);
                    }
                }
            }
        }
    }

    #[gpui::test]
    fn live_tabs_two_panel_cluster_keeps_every_offscreen_tab_visible_and_clickable(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for i in 0..8 {
                w.push_test_panel(&format!("session-{i}"), cx);
                w.slots[i].width_fraction = 0.5;
                w.slots[i].animated_width = AnimatedValue::new(0.5, Duration::ZERO);
            }
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [1440.0, 1000.0, 640.0] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.0)));
            vcx.run_until_parked();
            for selected in [3, 4, 0, 7, 1, 6, 2, 5] {
                workspace.update(vcx, |w, cx| {
                    w.camera_x[w.active_row] = w.camera_target[w.active_row];
                    w.camera_started[w.active_row] = None;
                    w.live_tabs.settle();
                    cx.notify();
                });
                vcx.run_until_parked();
                let track = vcx.debug_bounds("live-session-tabs").unwrap();
                let targets = workspace.read_with(vcx, |w, _| w.live_tabs.hit_targets().to_vec());
                assert_eq!(targets.len(), 8, "off-screen panels must retain their tabs");
                for &(index, x) in &targets {
                    let tab = vcx
                        .debug_bounds(Box::leak(
                            format!("live-session-tab-{index}").into_boxed_str(),
                        ))
                        .unwrap();
                    assert!(tab.left() >= track.left() - px(0.01));
                    assert!(tab.right() <= track.right() + px(0.01));
                    assert!(tab.size.width > px(0.0));
                    assert!(track.left() + px(x) > tab.left());
                    assert!(track.left() + px(x) < tab.right());
                }
                let x = targets
                    .iter()
                    .find(|(index, _)| *index == selected)
                    .unwrap()
                    .1;
                vcx.simulate_click(
                    gpui::point(track.left() + px(x), track.bottom() - px(12.0)),
                    gpui::Modifiers::default(),
                );
                vcx.run_until_parked();
                assert_eq!(
                    workspace.read_with(vcx, |w, _| w.active),
                    selected,
                    "window={width}, selected={selected}, track={track:?}, targets={targets:?}, camera={:?}",
                    workspace.read_with(vcx, |w, _| (w.camera_x, w.camera_target, w.camera_dirty))
                );
            }
        }
    }

    #[gpui::test]
    fn live_tabs_four_panel_group_stays_left_aligned_when_focus_changes(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for i in 0..4 {
                w.push_test_panel(&format!("panel-{i}"), cx);
                w.slots[i].width_fraction = 0.25;
                w.slots[i].animated_width = AnimatedValue::new(0.25, Duration::ZERO);
            }
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(1600.0), px(700.0)));
        vcx.run_until_parked();
        let selectors = [
            "live-session-tab-0",
            "live-session-tab-1",
            "live-session-tab-2",
            "live-session-tab-3",
        ];
        let initial = selectors.map(|selector| vcx.debug_bounds(selector).unwrap());
        let track = vcx.debug_bounds("live-session-tabs").unwrap();
        assert_eq!(initial[0].left() - px(TAB_GROUP_LABEL_WIDTH), track.left());
        for pair in initial.windows(2) {
            assert_eq!(pair[1].left() - pair[0].right(), px(TAB_GAP));
        }
        for selected in [1, 2, 3, 0] {
            vcx.simulate_click(initial[selected].center(), gpui::Modifiers::default());
            vcx.simulate_mouse_move(
                gpui::point(px(5.0), px(600.0)),
                None,
                gpui::Modifiers::default(),
            );
            vcx.run_until_parked();
            workspace.update(vcx, |w, cx| {
                w.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.active), selected);
            for (i, selector) in selectors.iter().enumerate() {
                let tab = vcx.debug_bounds(selector).unwrap();
                assert!((tab.left() - initial[i].left()).abs() <= px(12.01));
                let panel = vcx
                    .debug_bounds(["panel-0", "panel-1", "panel-2", "panel-3"][i])
                    .unwrap();
                if cfg!(target_os = "macos") {
                    assert_eq!(tab.center().y, track.top() + px(TAB_HEIGHT / 2.0));
                }
                assert!(tab.bottom() + px(TAB_FLOAT_GAP) <= panel.top());
            }
        }
    }

    /// Tab and camera targets must already be final while the closing surface
    /// is still mounted. Retirement must not trigger a second layout move.
    #[gpui::test]
    fn closing_tabs_target_survivors_before_surface_retirement(cx: &mut gpui::TestAppContext) {
        for initial in [0, 2, 4] {
            let (workspace, vcx) = cx.add_window_view(|_, cx| {
                let mut w = Workspace::for_test(learning::Coach::new(), cx);
                w.show_sidebar = false;
                w.show_minimap = false;
                for i in 0..5 {
                    w.push_test_panel(&format!("panel-{i}"), cx);
                    w.slots[i].width_fraction = 1.0;
                    w.slots[i].close_progress = AnimatedValue::new(1.0, Duration::from_secs(60));
                }
                w.set_active(initial, cx);
                w
            });
            vcx.update(|window, cx| {
                workspace.update(cx, |w, cx| {
                    w.resolve_camera_target(1200.0);
                    w.camera_x[0] = w.camera_target[0];
                    let before_camera = w.camera_target[0];
                    let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                    w.live_tabs.settle();
                    let closed_id = w.slots[initial].panel.entity_id().as_u64();
                    w.close_panel(&ClosePanel, window, cx);
                    w.resolve_camera_target(1200.0);
                    let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                    assert_eq!(w.slots.len(), 5, "surface is still fading");
                    assert_eq!(w.live_tabs.tabs.len(), 4);
                    assert!(!w.live_tabs.tabs.contains_key(&closed_id));
                    assert!(w.live_tabs.hit_targets().iter().all(|(i, _)| *i != initial));
                    assert!(
                        w.camera_target[0] <= before_camera + 0.01,
                        "closing must not pan right toward the successor's old position"
                    );
                    let targets: HashMap<_, _> = w
                        .live_tabs
                        .tabs
                        .iter()
                        .map(|(id, tab)| (*id, tab.target))
                        .collect();
                    let camera = w.camera_target[0];
                    w.remove_finished_closing_panels(
                        Instant::now() + Duration::from_secs(61),
                        window,
                        cx,
                    );
                    w.resolve_camera_target(1200.0);
                    let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                    assert_eq!(w.slots.len(), 4);
                    assert_eq!(w.camera_target[0], camera, "no second camera move");
                    for (id, tab) in &w.live_tabs.tabs {
                        assert_eq!(tab.target, targets[id], "no second tab move");
                    }
                    assert!(!w.slots[w.active].closing);
                    assert!(
                        w.slots[w.active]
                            .panel
                            .read(cx)
                            .input_focus_handle(cx)
                            .is_focused(window)
                    );
                });
            });
        }
    }

    #[gpui::test]
    fn closing_final_tab_shows_empty_workspace_before_surface_retirement(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.push_test_panel("last", cx);
            w.slots[0].close_progress = AnimatedValue::new(1.0, Duration::from_secs(60));
            w
        });
        vcx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.close_panel(&ClosePanel, window, cx);
                let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                assert_eq!(w.slots.len(), 1);
                assert!(w.live_tabs.hit_targets().is_empty());
                assert_eq!(w.live_tabs.tabs.len(), 1);
                assert!(w.live_tabs.tabs.contains_key(&u64::MAX));
                assert!(w.focus_handle.is_focused(window));
            });
        });
    }

    #[gpui::test]
    fn live_tabs_stay_still_while_the_panel_camera_moves(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            for i in 0..4 {
                w.push_test_panel(&format!("panel-{i}"), cx);
            }
            w
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(1200.0), px(700.0)));
        vcx.run_until_parked();
        let selectors = [
            "live-session-tab-0",
            "live-session-tab-1",
            "live-session-tab-2",
            "live-session-tab-3",
        ];
        let initial = selectors.map(|selector| vcx.debug_bounds(selector).unwrap());
        let panel_left = vcx.debug_bounds("panel-1").unwrap().left();
        for camera in [80.0, 160.0, 480.0] {
            workspace.update(vcx, |w, cx| {
                w.camera_x[0] = camera;
                w.camera_target[0] = camera;
                w.camera_started[0] = None;
                w.camera_dirty[0] = false;
                cx.notify();
            });
            vcx.run_until_parked();
            for (i, selector) in selectors.iter().enumerate() {
                assert_eq!(vcx.debug_bounds(selector).unwrap(), initial[i]);
            }
            assert!(
                (panel_left - vcx.debug_bounds("panel-1").unwrap().left() - px(camera)).abs()
                    < px(0.01)
            );
        }
    }

    #[test]
    fn live_tabs_motion_expands_lifts_and_retargets_without_jumping() {
        let now = Instant::now();
        let duration = Duration::from_millis(150);
        let layout = TabLayout::new(512.0, 12);
        let targets = |selected| {
            (0..12)
                .map(|index| (index as u64, layout.geometry(index, selected)))
                .collect::<Vec<_>>()
        };
        let mut motion = TabMotion::default();
        let initial = motion.sample(&targets(0), 512.0, duration, now);
        assert!(!motion.is_animating());
        let start = motion.sample(&targets(5), 512.0, duration, now);
        assert_eq!(initial, start);
        assert!(motion.is_animating());
        let middle = motion.sample(
            &targets(5),
            512.0,
            duration,
            now + Duration::from_millis(50),
        );
        assert!(middle[5].width > initial[5].width && middle[5].width < 208.0);
        assert!(middle[5].height > TAB_HEIGHT - 4.0 && middle[5].height < TAB_HEIGHT);
        assert!(middle[5].left < initial[5].left);
        let reversal = motion.sample(
            &targets(0),
            512.0,
            duration,
            now + Duration::from_millis(50),
        );
        assert_eq!(middle, reversal);
        let settled = motion.sample(
            &targets(0),
            512.0,
            duration,
            now + Duration::from_millis(250),
        );
        assert_eq!(settled, initial);
        assert!(!motion.is_animating());
        let reduced = motion.sample(&targets(5), 512.0, Duration::ZERO, now);
        assert_eq!(reduced[5], layout.geometry(5, 5));
        assert!(!motion.is_animating());
        let smaller = TabLayout::new(192.0, 12);
        let targets: Vec<_> = (0..12)
            .map(|i| (i as u64, smaller.geometry(i, 5)))
            .collect();
        let resized = motion.sample(&targets, 192.0, duration, now);
        assert!(!motion.is_animating());
        assert!(
            resized
                .iter()
                .all(|tab| tab.left >= 0.0 && tab.left + tab.width <= 192.001)
        );
    }

    #[gpui::test]
    fn live_tabs_switch_rows_and_stay_detached_from_panels(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.show_sidebar = false;
            for name in ["First session", "Second session", "Another workspace"] {
                workspace.push_test_panel(name, cx);
            }
            workspace.slots[2].row = 1;
            workspace
        });
        vcx.run_until_parked();
        let first = vcx.debug_bounds("live-session-tab-0").unwrap();
        let other = vcx.debug_bounds("live-session-tab-2").unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(first.center().y, other.center().y);
        } else {
            assert_eq!(first.bottom(), other.bottom());
        }
        assert_eq!(first.size.height - other.size.height, px(4.0));
        assert_eq!(
            first.bottom() + px(TAB_FLOAT_GAP),
            vcx.debug_bounds("panel-0").unwrap().top()
        );
        assert!(vcx.debug_bounds("workspace-bar").is_none());
        vcx.simulate_click(other.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, _| {
            assert_eq!(workspace.active, 2);
            assert_eq!(workspace.active_row, 1);
            assert!(!workspace.overview);
        });
        assert!(vcx.debug_bounds("live-session-tab-2-focused").is_some());
        assert!(vcx.debug_bounds("live-session-tab-0-focused").is_none());
    }

    #[gpui::test]
    fn live_tabs_remain_reachable_during_resize_and_keyboard_navigation(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            for index in 0..12 {
                workspace.push_test_panel(&format!("A long session title number {index}"), cx);
            }
            workspace
        });
        let handle = vcx.update(|window, cx| {
            window.focus(&workspace.read(cx).focus_handle.clone(), cx);
            window.window_handle()
        });
        vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("super-p");
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active), 11);
        let track = vcx.debug_bounds("live-session-tabs").unwrap();
        let selected = vcx.debug_bounds("live-session-tab-11").unwrap();
        assert!(selected.left() >= track.left());
        assert!(
            selected.right() <= track.right() + px(1.),
            "selected={selected:?}, track={track:?}"
        );
        workspace.update(vcx, |w, cx| {
            // Finish both independent animations before checking selection.
            w.camera_x[w.active_row] = w.camera_target[w.active_row];
            w.camera_started[w.active_row] = None;
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        assert_eq!(
            vcx.debug_bounds("live-session-tab-11").unwrap().size.width,
            px(TabLayout::grouped(f32::from(track.size.width), &[0; 12], 11)[11].width)
        );
        vcx.simulate_keystrokes("super-u");
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            // Finish both independent animations before checking selection.
            w.camera_x[w.active_row] = w.camera_target[w.active_row];
            w.camera_started[w.active_row] = None;
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        let first = vcx.debug_bounds("live-session-tab-0").unwrap();
        assert!(first.left() >= track.left());
        vcx.update(|window, cx| window.simulate_mouse_move(first.center(), cx));
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("live-session-tab-tooltip").is_some());
        for width in [1440., 800., 640., 480., 1000.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            vcx.run_until_parked();
            // Resize retargets camera/tab motion. Click settled geometry rather
            // than bounds from an animation frame that can move before dispatch.
            workspace.update(vcx, |workspace, cx| {
                workspace.camera_x[workspace.active_row] =
                    workspace.camera_target[workspace.active_row];
                workspace.camera_started[workspace.active_row] = None;
                workspace.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
            let track = vcx.debug_bounds("live-session-tabs").unwrap();
            let mut hidden = Vec::new();
            for index in 0..12 {
                let Some(tab) = vcx.debug_bounds(format!("live-session-tab-{index}").leak()) else {
                    hidden.push(index);
                    continue;
                };
                // Crowded folders retain exposed edges inside the centered group.
                assert!(tab.left() >= track.left());
                assert!(
                    tab.right() <= track.right() + px(1.),
                    "window={width} index={index} tab={tab:?} track={track:?}"
                );
                assert!(tab.size.width > px(0.) && tab.size.width <= px(208.));
                assert!(
                    vcx.debug_bounds(Box::leak(
                        format!("live-session-tab-{index}-emoji").into_boxed_str()
                    ))
                    .is_some()
                );
            }
            let last = vcx.debug_bounds("live-session-tab-11").unwrap();
            let plus = vcx.debug_bounds("tab-new-session").unwrap();
            assert!(plus.left() >= last.right());
            assert!(vcx.debug_bounds("edge-new-session").is_none());
            let previous = vcx.debug_bounds("live-session-tab-10").unwrap();
            let x = track.left()
                + px(workspace.read_with(vcx, |w, _| {
                    w.live_tabs
                        .hit_targets
                        .iter()
                        .find(|(i, _)| *i == 11)
                        .unwrap()
                        .1
                }));
            vcx.simulate_click(gpui::point(x, last.center().y), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(
                workspace.read_with(vcx, |w, _| w.active),
                11,
                "width={width}, last={last:?}, previous={previous:?}, x={x:?}"
            );
            workspace.update_in(vcx, |workspace, window, cx| {
                assert_eq!(workspace.navigation_state(window, cx)["keyboard_panel"], 11);
                assert_eq!(workspace.test_coach().trace("new_panel").slow_paths, 0);
            });
            if width > 480. {
                assert!(hidden.is_empty(), "window={width} hid tabs {hidden:?}");
            }
            if let Some(trigger) = vcx.debug_bounds("tab-overflow") {
                assert!(!hidden.is_empty());
                vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
                vcx.run_until_parked();
                for index in 0..12 {
                    assert_eq!(
                        vcx.debug_bounds(format!("tab-overflow-item-{index}").leak())
                            .is_some(),
                        hidden.contains(&index),
                        "window={width} overflow availability for session {index}"
                    );
                }
                let index = hidden[0];
                let item = vcx
                    .debug_bounds(format!("tab-overflow-item-{index}").leak())
                    .unwrap();
                vcx.simulate_click(item.center(), gpui::Modifiers::default());
                vcx.run_until_parked();
                assert_eq!(workspace.read_with(vcx, |w, _| w.active), index);
                assert!(
                    vcx.debug_bounds(format!("live-session-tab-{index}").leak())
                        .is_some()
                );
                assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
                vcx.simulate_keystrokes("super-p");
                vcx.run_until_parked();
                assert_eq!(workspace.read_with(vcx, |w, _| w.active), 11);
            } else {
                assert!(hidden.is_empty(), "tabs {hidden:?} are unreachable");
            }
        }
    }

    #[gpui::test]
    fn overflow_keeps_workspace_labels_and_promotes_the_chosen_session(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            for index in 0..16 {
                workspace.push_test_panel(&format!("Overflow session {index}"), cx);
                workspace.slots[index].row = index / 8;
            }
            workspace
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(420.0), px(700.0)));
        vcx.run_until_parked();
        let trigger = vcx.debug_bounds("tab-overflow").unwrap();
        assert!(trigger.right() <= vcx.debug_bounds("workspace-files-toggle").unwrap().left());
        assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
        assert!(vcx.debug_bounds("workspace-tab-group-label-1").is_some());
        let hidden = workspace.read_with(vcx, |w, _| {
            (0..16)
                .find(|index| {
                    !w.live_tabs
                        .hit_targets
                        .iter()
                        .any(|(visible, _)| visible == index)
                })
                .unwrap()
        });
        vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tab-overflow-menu").is_some());
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
        vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let item = vcx
            .debug_bounds(format!("tab-overflow-item-{hidden}").leak())
            .unwrap();
        vcx.simulate_click(item.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
        assert_eq!(workspace.read_with(vcx, |w, _| w.active), hidden);
        assert!(
            vcx.debug_bounds(format!("live-session-tab-{hidden}").leak())
                .is_some()
        );
    }

    #[gpui::test]
    fn live_tabs_reserve_space_in_normal_mode_and_beside_minimap(cx: &mut gpui::TestAppContext) {
        let (_, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.layout_mode = crate::config::LayoutMode::Normal;
            workspace.enable_test_minimap();
            for index in 0..24 {
                workspace.push_test_panel(&format!("Session {index}"), cx);
            }
            workspace
        });
        vcx.run_until_parked();
        let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
        let minimap = vcx.debug_bounds("minimap").unwrap();
        if cfg!(target_os = "macos") {
            assert!(tabs.bottom() < minimap.top());
        } else {
            assert!(tabs.right() < minimap.left());
        }
        assert!(tabs.bottom() <= vcx.debug_bounds("panel-0").unwrap().top());
        for index in 0..24 {
            let tab = vcx
                .debug_bounds(Box::leak(
                    format!("live-session-tab-{index}").into_boxed_str(),
                ))
                .unwrap();
            assert!(tab.left() >= tabs.left());
            assert!(tab.right() <= tabs.right() + px(1.));
        }
    }

    #[gpui::test]
    fn empty_workspace_does_not_mark_another_rows_session_as_focused(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel("Session", cx);
            workspace.select_row(2, 0);
            workspace
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("live-session-empty-tab").is_some());
        assert!(vcx.debug_bounds("live-session-tab-0-focused").is_none());
        let tab = vcx.debug_bounds("live-session-tab-0").unwrap();
        vcx.simulate_click(tab.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 0);
        assert!(vcx.debug_bounds("live-session-empty-tab").is_none());
        assert!(vcx.debug_bounds("live-session-tab-0-focused").is_some());
    }
}
