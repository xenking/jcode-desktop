//! Pure arithmetic of the strip: widths, insertion, focus and camera.
use super::*;

/// The first panel owns the viewport. Later panels use the normal column width
/// so opening one reveals the scrolling layout rather than covering it.
pub(super) fn spawned_panel_width(existing_panels: usize) -> f32 {
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
pub(super) fn demoted_width(width: f32, panels_after_spawn: usize) -> f32 {
    if panels_after_spawn == 2 && width >= 1.0 {
        DEFAULT_WIDTH
    } else {
        width
    }
}

/// A one-line description of the strip layout, for tests and for the
/// `JCODE_DESKTOP_STATE` debug dump: `strip=<row> focus=<pos> widths=a,b,c`.
pub(super) fn describe_strip(widths: &[f32], focus_position: Option<usize>, row: usize) -> String {
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
pub(super) fn describe_coach(mastery: f32, saved: u32, wasted: u32, teaching: Option<&'static str>) -> String {
    format!(
        "coach mastery={:.2} saved={saved} wasted={wasted} teaching={}",
        mastery,
        teaching.unwrap_or("-")
    )
}

/// Shared cubic ease-out used by the strip camera.
pub(super) fn ease_out_cubic(t: f32) -> f32 {
    transition::ease_out_cubic(t)
}

/// Where a newly created panel is inserted: directly right of the focused
/// panel when it is on this strip, otherwise after the strip's last panel.
/// `row_last` is the index of the strip's rightmost panel, if any.
/// A click that jumps this many panels or more, landing on an end of the strip,
/// is the work `super-home`/`super-end` exists for. Shorter hops are ordinary
/// left/right navigation.
pub(super) const LONG_HOP: usize = 3;

/// Which shortcut a click-to-focus bypassed, given how far it jumped and where
/// it landed.
pub(super) fn click_skill(steps: usize, landed_at: usize, strip_len: usize) -> &'static str {
    let at_end = landed_at == 0 || landed_at + 1 == strip_len;
    if steps >= LONG_HOP && at_end {
        "focus_first_last"
    } else {
        "focus_left_right"
    }
}

pub(super) fn insert_index(
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
pub(super) fn focus_after_close(closed: usize, remaining: &[usize]) -> usize {
    match remaining.iter().find(|&&index| index >= closed) {
        Some(&right) => right,
        None => remaining.last().copied().unwrap_or(0),
    }
}

/// niri `switch-preset-column-width`: the next preset above `current`,
/// wrapping back to the narrowest.
pub(super) fn next_preset(current: f32) -> f32 {
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
pub(super) fn toggle_maximize(width: f32, restore: Option<f32>) -> (f32, Option<f32>) {
    match restore {
        Some(restore) => (restore, None),
        None if width >= 1.0 => (DEFAULT_WIDTH, None),
        None => (1.0, Some(width)),
    }
}

/// Whether a strip panel spanning `left..left + width` (strip coordinates)
/// overlaps the viewport when the strip is scrolled to `camera_x`.
pub(super) fn strip_panel_visible(left: f32, width: f32, camera_x: f32, viewport: f32) -> bool {
    let start = left - camera_x;
    start < viewport && start + width > 0.0
}

/// niri `center-focused-column "never"`: scroll the least amount that brings
/// `[left, left + width]` fully into a `viewport`-wide window at `current`.
pub(super) fn scroll_into_view(current: f32, left: f32, width: f32, viewport: f32) -> f32 {
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
pub(super) fn pan_camera(current: f32, delta: f32, total_width: f32, viewport: f32) -> f32 {
    let max_scroll = (total_width - viewport).max(-GAP).max(-STRUT);
    (current + delta).clamp(-STRUT, max_scroll)
}

pub(super) fn smoothed_camera_position(from: f32, target: f32, elapsed: Duration, duration: Duration) -> f32 {
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
pub(super) fn minimap_scale(
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
pub(super) fn gesture_alpha(since_last_delta: Duration) -> Option<f32> {
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
pub(super) fn panel_at_viewport_center(
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
impl Workspace {
    pub(super) fn width_for_fraction(fraction: f32, viewport: f32) -> f32 {
        // Narrow windows show one readable conversation without overwriting the
        // user's width presets. Widening restores the original arrangement.
        let fraction = if viewport < SINGLE_PANEL_BREAKPOINT {
            1.0
        } else {
            fraction
        };
        ((viewport - STRUT * 2.0) * fraction - GAP)
            .max(320.0_f32.min((viewport - STRUT * 2.0).max(1.0)))
    }
}

pub(super) const COMPACT_BREAKPOINT: f32 = 1100.0;

pub(super) fn is_compact(width: f32) -> bool {
    width < COMPACT_BREAKPOINT
}
