//! Labeled live-session folder tabs, independent of the sidebar's navigation.
use super::sidebar_atoms::tab_tooltip;
use super::*;
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
                    .data(include_bytes!("../../../../../../assets/icons/pencil.svg"))
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
#[path = "live_tabs_tests.rs"]
mod tests;
