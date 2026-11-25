//! Counted filesystem probes for code that feeds rendering.
//!
//! Render must read in-memory snapshots only. Probes that answer "does this
//! path exist" or "which branch is checked out" go through these wrappers so a
//! test can measure how many ran on the calling thread during one frame.
//! Counters and the test clock are per thread, so parallel tests and
//! background workers never see each other's numbers.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

thread_local! {
    static PROBES: Cell<u64> = const { Cell::new(0) };
    static BACKGROUND_PROBES: Cell<u64> = const { Cell::new(0) };
    static OFF_MAIN: Cell<bool> = const { Cell::new(false) };
    static SKEW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// Count a probe made by code that cannot use the wrappers below, such as a
/// loader that reads several files.
pub fn note() {
    let counter = if OFF_MAIN.with(Cell::get) {
        &BACKGROUND_PROBES
    } else {
        &PROBES
    };
    counter.with(|probes| probes.set(probes.get() + 1));
}

/// Probes made so far on this thread, outside [`off_main`].
pub fn count() -> u64 {
    PROBES.with(Cell::get)
}

/// Probes made so far inside [`off_main`] on this thread. Headless tests run
/// background work on the test thread, so this keeps it apart from render.
pub fn background_count() -> u64 {
    BACKGROUND_PROBES.with(Cell::get)
}

/// Run a probe on a background executor. Marks the work so tests can tell it
/// from probes that blocked the thread rendering a frame.
pub fn off_main<R>(probe: impl FnOnce() -> R) -> R {
    let previous = OFF_MAIN.with(|flag| flag.replace(true));
    let result = probe();
    OFF_MAIN.with(|flag| flag.set(previous));
    result
}

/// The clock probe caches use for freshness. Tests advance it instead of
/// sleeping through a TTL.
pub fn now() -> Instant {
    Instant::now() + SKEW.with(Cell::get)
}

/// Make every cache on this thread see `by` more time as elapsed.
pub fn advance_clock(by: Duration) {
    SKEW.with(|skew| skew.set(skew.get() + by));
}

pub fn is_file(path: &Path) -> bool {
    note();
    path.is_file()
}

pub fn is_dir(path: &Path) -> bool {
    note();
    path.is_dir()
}

pub fn exists(path: &Path) -> bool {
    note();
    path.exists()
}

pub fn read_to_string(path: &Path) -> std::io::Result<String> {
    note();
    std::fs::read_to_string(path)
}

pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    note();
    std::fs::canonicalize(path)
}
