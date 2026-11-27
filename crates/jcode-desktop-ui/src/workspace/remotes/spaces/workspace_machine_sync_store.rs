//! What each machine's daemon and this Desktop last agreed on, kept on disk
//! beside the crash-recovery snapshot. The snapshot is rewritten whole by every
//! Desktop build, including ones that know nothing about syncing, so this
//! lives in a file of its own: a draft such an older build edited still
//! differs from what was agreed, and is written to the daemon, not replaced.
use jcode_desktop_model::machine_state::Fingerprints;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The machine's key in the file: `local` for this one, else its paired host.
pub(super) fn key(host: Option<&str>) -> String {
    host.unwrap_or("local").to_owned()
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct File {
    v: u32,
    machines: BTreeMap<String, BTreeMap<String, u64>>,
}

pub(super) struct Store {
    path: PathBuf,
    file: File,
    dirty: bool,
}

impl Store {
    /// An unreadable or foreign file counts as nothing agreed yet: every draft
    /// then reads as changed here, which only ever writes, never discards.
    pub(super) fn open(path: PathBuf) -> Self {
        let file = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .filter(|file| file.v == 1)
            .unwrap_or(File { v: 1, machines: BTreeMap::new() });
        Self { path, file, dirty: false }
    }

    pub(super) fn agreed(&self, host: Option<&str>) -> Fingerprints {
        self.file.machines.get(&key(host)).cloned().unwrap_or_default()
    }

    pub(super) fn record(&mut self, host: Option<&str>, applied: &Fingerprints) {
        if self.file.machines.get(&key(host)) != Some(applied) {
            self.file.machines.insert(key(host), applied.clone());
            self.dirty = true;
        }
    }

    /// Write the file when something changed. Atomic, owner-only.
    pub(super) fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = write(&self.path, &self.file).is_err();
    }
}

fn write(path: &Path, file: &File) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut out = std::fs::OpenOptions::new();
    out.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut out, 0o600);
    let mut handle = out.open(&tmp)?;
    handle.write_all(&serde_json::to_vec(file)?)?;
    handle.sync_all()?;
    std::fs::rename(&tmp, path)
}
