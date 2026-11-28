//! Detect persisted edit-stat changes without increasing full-catalog polling.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

type Stamp = BTreeMap<PathBuf, (u64, Option<SystemTime>)>;

/// How often a focused catalog looks for edit-stat changes.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Polls the edit-stat sidecars only while the catalog is in the foreground.
pub(in crate::harness) struct EditWatch {
    home: Option<PathBuf>,
    stamp: Option<Stamp>,
    next_check: Option<Instant>,
}

impl EditWatch {
    pub(in crate::harness) fn new() -> Self {
        Self {
            home: crate::platform::jcode_home(),
            stamp: None,
            next_check: None,
        }
    }

    /// Start or stop polling. The previous stamp is kept, so edits made while
    /// hidden are noticed on activation without scanning in the background.
    pub(in crate::harness) fn set_foreground(&mut self, focused: bool) {
        self.next_check = focused.then(Instant::now);
    }

    /// When the next check is due, if polling is on.
    pub(in crate::harness) fn next_check(&self) -> Option<Instant> {
        self.next_check
    }

    /// Check if due. True when the sidecars changed since the last check.
    pub(in crate::harness) fn poll(&mut self, now: Instant) -> bool {
        if !self.next_check.is_some_and(|deadline| now >= deadline) {
            return false;
        }
        self.next_check = Some(now + CHECK_INTERVAL);
        let Some(current) = self.home.as_deref().and_then(stamp) else {
            return false;
        };
        let changed = self
            .stamp
            .as_ref()
            .is_some_and(|previous| *previous != current);
        self.stamp = Some(current);
        changed
    }
}

fn stamp(home: &Path) -> Option<Stamp> {
    let dir = home.join("sessions/edit-stats");
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Stamp::new()),
        Err(_) => return None,
    };
    let mut result = Stamp::new();
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let meta = entry.metadata().ok()?;
        if meta.is_file() {
            result.insert(path, (meta.len(), meta.modified().ok()));
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_existing_sidecar_updates_without_a_directory_change() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("sessions/edit-stats");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session-test.json");
        std::fs::write(&file, b"+3/-0").unwrap();
        let before = stamp(home.path()).unwrap();
        std::fs::write(&file, b"+6/-2 and more").unwrap();
        assert_ne!(before, stamp(home.path()).unwrap());
    }
}
