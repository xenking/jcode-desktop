//! The active theme of Jcode Desktop: which [`Palette`] is selected, the
//! fade between two selections, and the lookups views make on every frame.
//!
//! Layers, bottom to top: [`palette`] is plain color data, [`presets`] builds
//! the built-in palettes and [`preset`] names them, and this module chooses
//! among them, applying the user's configuration. Nothing below this module
//! knows what is selected.

mod palette;
mod preset;
mod presets;
#[cfg(test)]
mod tests;

pub use palette::{Ink, Palette, to_hsla};
pub use preset::ThemePreset;

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

/// The process-wide selected theme. A unit type: the state it fronts is a
/// handful of statics, and callers only ever ask it for the [`Palette`] in
/// effect.
pub struct Theme;

// Publish palette and transition status together, so stable readers cannot
// observe a new target before its fade is installed. No extra cache is needed.
const TRANSITIONING: usize = 1 << (usize::BITS - 1);
static ACTIVE_THEME: AtomicUsize = AtomicUsize::new(ThemePreset::PureBlack.index());

impl Theme {
    /// The palette to paint with right now, including a fade in progress.
    pub fn global() -> &'static Palette {
        let palettes = themes();
        let active = ACTIVE_THEME.load(Ordering::Acquire);
        if active & TRANSITIONING == 0 {
            return &palettes[active];
        }
        let mut transition = transition_state().lock().unwrap();
        // Reload under the selection lock, never pairing an old target with a
        // concurrently selected source palette.
        let target = ACTIVE_THEME.load(Ordering::Relaxed) & !TRANSITIONING;
        if let Some((from, started)) = *transition {
            let progress = started.elapsed().as_secs_f32() / 0.18;
            if progress < 1.0 {
                let step = (progress * 16.0).round().clamp(0.0, 16.0) as usize;
                return &transition_frames(from, target)[step];
            }
            *transition = None;
            ACTIVE_THEME.store(target, Ordering::Release);
        }
        &palettes[target]
    }

    /// Final palette for expensive cached artwork. Transition frames should not
    /// trigger repeated diagram layout and path tessellation while fading UI.
    pub fn selected() -> &'static Palette {
        let _ = themes();
        &themes()[ACTIVE_THEME.load(Ordering::Acquire) & !TRANSITIONING]
    }

    pub fn active_preset() -> ThemePreset {
        let _ = themes();
        ThemePreset::ALL[ACTIVE_THEME.load(Ordering::Acquire) & !TRANSITIONING]
    }

    pub fn select(preset: ThemePreset) {
        // The fade runs on wall-clock time, which GPUI's test executor cannot
        // advance, so each switch kept redrawing for its full 180 ms. The
        // palette is also process-global, so a fade started by one test leaked
        // in-between colours into tests running in parallel. Switch instantly.
        let animate = !(cfg!(any(test, feature = "test-support"))
            || crate::config::get().appearance.reduce_motion);
        select_theme(preset.index(), animate);
    }

    /// Settled palette for a preset, for swatch previews that should not
    /// depend on the active theme.
    pub fn preview(preset: ThemePreset) -> &'static Palette {
        &themes()[preset.index()]
    }

    /// Window background the settled palette needs.
    pub fn window_background() -> gpui::WindowBackgroundAppearance {
        if Self::selected().is_translucent() {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Opaque
        }
    }

    /// Apply the palette's window background when it changes. `applied`
    /// remembers the last value per window so frames do not re-commit the
    /// compositor blur region.
    pub fn sync_window_background(
        window: &gpui::Window,
        applied: &mut Option<gpui::WindowBackgroundAppearance>,
    ) {
        let wanted = Self::window_background();
        if *applied != Some(wanted) {
            window.set_background_appearance(wanted);
            *applied = Some(wanted);
        }
    }

    pub fn is_transitioning() -> bool {
        ACTIVE_THEME.load(Ordering::Acquire) & TRANSITIONING != 0
    }
}

fn select_theme(target: usize, animate: bool) {
    let _ = themes();
    let mut transition = transition_state().lock().unwrap();
    let from = ACTIVE_THEME.load(Ordering::Relaxed) & !TRANSITIONING;
    *transition = animate.then(|| (from, std::time::Instant::now()));
    ACTIVE_THEME.store(
        target | if animate { TRANSITIONING } else { 0 },
        Ordering::Release,
    );
}

/// The active theme is process-global. Every unit test that selects a theme
/// must hold this for its whole body, or parallel tests observe its palette.
#[cfg(any(test, feature = "test-support"))]
pub fn test_theme_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn transition_state() -> &'static Mutex<Option<(usize, std::time::Instant)>> {
    static STATE: OnceLock<Mutex<Option<(usize, std::time::Instant)>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

/// The 17 interpolated palettes for one theme change, computed on first use.
/// A session visits only a few of the 18 x 18 preset pairs, so building all
/// 5,508 palettes on the first switch was wasted work.
fn transition_frames(from: usize, to: usize) -> &'static [Palette] {
    const N: usize = ThemePreset::ALL.len();
    static FRAMES: OnceLock<Vec<OnceLock<Box<[Palette]>>>> = OnceLock::new();
    let pairs = FRAMES.get_or_init(|| (0..N * N).map(|_| OnceLock::new()).collect());
    pairs[from * N + to].get_or_init(|| {
        (0..=16)
            .map(|step| themes()[from].mixed(&themes()[to], step as f32 / 16.0))
            .collect()
    })
}

/// Every preset with the user's configuration applied. The first call also
/// selects the configured theme.
fn themes() -> &'static [Palette; ThemePreset::ALL.len()] {
    static THEMES: OnceLock<[Palette; ThemePreset::ALL.len()]> = OnceLock::new();
    THEMES.get_or_init(|| {
        let appearance = crate::config::appearance();
        let result = presets::builtin().map(|palette| palette.configured(&appearance));
        ACTIVE_THEME.store(
            ThemePreset::from_id(&appearance.theme).index(),
            Ordering::Relaxed,
        );
        result
    })
}
