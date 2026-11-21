//! Detect persisted edit-stat changes without increasing full-catalog polling.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(super) type Stamp = BTreeMap<PathBuf, (u64, Option<SystemTime>)>;

pub(super) fn stamp(home: &Path) -> Option<Stamp> {
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
