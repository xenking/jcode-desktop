//! What each machine's daemon and this Desktop last agreed on, kept on disk
//! beside the crash-recovery snapshot. The snapshot is rewritten whole by every
//! Desktop build, including ones that know nothing about syncing, so this
//! lives in a file of its own: a draft such an older build edited still
//! differs from what was agreed, and is written to the daemon, not replaced.
use jcode_desktop_model::machine_state::Fingerprints;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The machine's key in the file: `local` for this one, else its paired host.
pub(super) fn key(host: Option<&str>) -> String {
    host.unwrap_or("local").to_owned()
}

/// What is kept for one machine. A file from before removals were recorded
/// holds only the fingerprints.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum Kept {
    Now {
        applied: BTreeMap<String, u64>,
        /// Entries this Desktop saw the user remove and has not yet told the
        /// daemon about: the only absences that may delete anything there.
        #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
        removed: BTreeSet<String>,
    },
    Before(BTreeMap<String, u64>),
}

impl Kept {
    fn split(&self) -> (Fingerprints, BTreeSet<String>) {
        match self {
            Kept::Now { applied, removed } => (applied.clone(), removed.clone()),
            Kept::Before(applied) => (applied.clone(), BTreeSet::new()),
        }
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct File {
    v: u32,
    machines: BTreeMap<String, Kept>,
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

    /// What was agreed with the machine, and what the user removed since.
    pub(super) fn kept(&self, host: Option<&str>) -> (Fingerprints, BTreeSet<String>) {
        self.file.machines.get(&key(host)).map(Kept::split).unwrap_or_default()
    }

    pub(super) fn record(&mut self, host: Option<&str>, applied: &Fingerprints, removed: &BTreeSet<String>) {
        let now = Kept::Now { applied: applied.clone(), removed: removed.clone() };
        let same = self.file.machines.get(&key(host)).map(Kept::split).is_some_and(|(a, r)| &a == applied && &r == removed);
        if !same {
            self.file.machines.insert(key(host), now);
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
