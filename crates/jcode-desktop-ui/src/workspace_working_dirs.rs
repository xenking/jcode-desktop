//! Working directories: defaults, usage ranking, and directory browsing.
use super::*;

/// The process default, for callers without a window (tests, helpers).
#[cfg(test)]
pub(super) fn default_working_dir() -> Option<String> {
    // Match `Workspace::for_test`, whose launch is empty. Reading this process's
    // environment made the expected default follow JCODE_DESKTOP_WORKING_DIR,
    // which sessions launched from Desktop inherit, so home-directory
    // assertions failed when tests ran from inside Desktop.
    launch_working_dir(&jcode_desktop_api::WindowLaunch::default())
}

/// `JCODE_DESKTOP_WORKING_DIR` from the launch that opened a window, so each
/// window of a shared host keeps the directory its shortcut asked for.
pub(super) fn launch_working_dir(launch: &jcode_desktop_api::WindowLaunch) -> Option<String> {
    launch_directory_override(launch).or_else(crate::platform_dirs::home_string)
}

pub(super) fn launch_directory_override(launch: &jcode_desktop_api::WindowLaunch) -> Option<String> {
    default_working_dir_from(
        launch.var("JCODE_DESKTOP_WORKING_DIR").map(str::to_owned),
        None,
        Path::is_dir,
    )
}

impl Workspace {
    pub(super) fn default_working_dir(&self) -> Option<String> {
        launch_working_dir(&self.launch)
    }

    pub(super) fn new_session_directory(&self) -> Option<String> {
        self.pinned_working_dir
            .as_deref()
            .filter(|dir| Path::new(dir).is_absolute() && Path::new(dir).is_dir())
            .map(str::to_owned)
            .or_else(|| self.default_working_dir())
    }
}

pub(super) fn default_working_dir_from(
    override_dir: Option<String>,
    home: Option<String>,
    is_dir: impl FnOnce(&Path) -> bool,
) -> Option<String> {
    override_dir
        .filter(|directory| {
            let path = Path::new(directory);
            path.is_absolute() && is_dir(path)
        })
        .or(home)
}

#[cfg(test)]
mod default_working_dir_tests {
    use super::default_working_dir_from;

    #[test]
    fn existing_absolute_override_takes_precedence() {
        for home in [Some("/home/user".to_owned()), None] {
            assert_eq!(
                default_working_dir_from(Some("/workspace".to_owned()), home, |_| true),
                Some("/workspace".to_owned())
            );
        }
    }

    #[test]
    fn missing_empty_or_relative_override_retains_home() {
        for directory in [None, Some(""), Some("relative"), Some("~/project")] {
            assert_eq!(
                default_working_dir_from(
                    directory.map(str::to_owned),
                    Some("/home/user".to_owned()),
                    |_| panic!("only absolute overrides should query the filesystem"),
                ),
                Some("/home/user".to_owned())
            );
        }
    }

    #[test]
    fn nonexistent_directory_or_file_retains_home_unchanged() {
        for home in [None, Some(String::new()), Some("relative-home".to_owned())] {
            assert_eq!(
                default_working_dir_from(Some("/not-a-directory".to_owned()), home.clone(), |_| {
                    false
                }),
                home
            );
        }
    }

    /// Windows of a shared single-panel host each keep the directory, args
    /// and state path of their own launch, including across a reload snapshot.
    #[test]
    fn each_window_launch_keeps_its_own_directory_and_arguments() {
        use super::{LaunchSnapshot, launch_working_dir};
        use jcode_desktop_api::{LaunchMode, WindowLaunch};
        let alpha = tempfile::tempdir().unwrap();
        let beta = tempfile::tempdir().unwrap();
        let launch = |dir: &std::path::Path, args: &[&str]| {
            WindowLaunch::from_parts(
                args.iter().copied(),
                [
                    (
                        "JCODE_DESKTOP_WORKING_DIR".into(),
                        dir.display().to_string(),
                    ),
                    (
                        "JCODE_DESKTOP_STATE".into(),
                        format!("{}/state", dir.display()),
                    ),
                ],
            )
        };
        let first = launch(alpha.path(), &["--single-panel"]);
        let second = launch(beta.path(), &["--single-panel", "--resume"]);
        assert_eq!(launch_working_dir(&first).as_deref(), alpha.path().to_str());
        assert_eq!(launch_working_dir(&second).as_deref(), beta.path().to_str());
        assert!(!LaunchMode::resume_requested(&first.args));
        assert!(LaunchMode::resume_requested(&second.args));
        let restored: WindowLaunch = LaunchSnapshot::from(&second).into();
        assert_eq!(restored, second);
        assert_eq!(
            launch_working_dir(&restored).as_deref(),
            beta.path().to_str()
        );
    }
}

/// Count session directories once. Lexical ties make the initial choice stable
/// even if the runtime returns the same history in a different order.
pub(super) fn most_used_working_dir(sessions: &[jcode_sdk::SessionInfo]) -> Option<String> {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for directory in sessions
        .iter()
        .filter(|session| !harness::is_remote_session(&session.session_id))
        .filter_map(|session| session.working_dir.as_deref())
    {
        if Path::new(directory).is_absolute() && Path::new(directory).is_dir() {
            *counts.entry(directory).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .min_by_key(|(directory, count)| (std::cmp::Reverse(*count), *directory))
        .map(|(directory, _)| directory.to_owned())
}

pub(super) fn filesystem_root() -> PathBuf {
    PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
}

pub(super) struct FileTree {
    pub(super) fallback: PathBuf,
    pub(super) root: Option<PathBuf>,
    pub(super) rows: HashMap<PathBuf, (Instant, std::sync::Arc<[FileRow]>)>,
    pub(super) dirty: HashSet<PathBuf>,
    pub(super) revision: u64,
    pub(super) reading: Option<PathBuf>,
    pub(super) needs_frame: bool,
}

impl Default for FileTree {
    fn default() -> Self {
        Self {
            fallback: std::env::current_dir().unwrap_or_else(|_| filesystem_root()),
            root: None,
            rows: HashMap::new(),
            dirty: HashSet::new(),
            revision: 0,
            reading: None,
            needs_frame: false,
        }
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct FileRow {
    pub(super) path: PathBuf,
    pub(super) label: gpui::SharedString,
    pub(super) is_dir: bool,
}

pub(super) fn read_file_rows(directory: &Path) -> std::sync::Arc<[FileRow]> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Default::default();
    };
    let mut rows = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let label = name.to_string_lossy();
            if jcode_desktop_model::context_files::hidden_explorer_entry(&name) {
                return None;
            }
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            Some((
                (!is_dir, name.to_ascii_lowercase()),
                FileRow {
                    path: entry.path(),
                    label: label.into_owned().into(),
                    is_dir,
                },
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|(key, _), (other, _)| key.cmp(other));
    rows.into_iter().map(|(_, row)| row).collect()
}

pub(super) fn contains_dot_directory(path: &Path, base: &Path) -> bool {
    path.strip_prefix(base).unwrap_or(path).components().any(|component| {
        matches!(component, std::path::Component::Normal(name) if name.to_string_lossy().starts_with('.'))
    })
}

pub(super) fn directory_entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if entry.file_name().to_string_lossy().starts_with('.') {
                return None;
            }
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
    Ok(entries)
}

pub(super) fn ranked_folder_matches_for_sessions(
    sessions: &[jcode_sdk::SessionInfo],
    base: &Path,
    query: &str,
) -> Vec<(PathBuf, String)> {
    let mut usage: HashMap<PathBuf, (usize, usize)> = HashMap::new();
    for (recency, session) in sessions.iter().rev().enumerate() {
        let Some(path) = session
            .working_dir
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| {
                path != base
                    && path.starts_with(base)
                    && !contains_dot_directory(path, base)
                    && path.is_dir()
            })
        else {
            continue;
        };
        let entry = usage.entry(path).or_insert((0, recency));
        entry.0 += 1;
        entry.1 = entry.1.min(recency);
    }

    let common_names = [
        "projects",
        "code",
        "dev",
        "src",
        "workspace",
        "documents",
        "desktop",
    ];
    let children = directory_entries(base).unwrap_or_default();
    let mut reasons = HashMap::<PathBuf, String>::new();
    let mut candidates = usage.keys().cloned().collect::<Vec<_>>();
    candidates.extend(children);
    if !query.is_empty()
        && let Ok(entries) = std::fs::read_dir(base)
    {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(query)
            {
                reasons.insert(
                    base.to_path_buf(),
                    format!("contains file · {}", entry.file_name().to_string_lossy()),
                );
                candidates.push(base.to_path_buf());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.retain(|path| {
        let searchable = path.to_string_lossy().to_lowercase();
        if !query.is_empty() {
            searchable.contains(query) || reasons.contains_key(path)
        } else {
            // An empty search is a real filesystem browser, not only a list of
            // guesses. Keeping every direct child lets the user walk from home
            // to its parent and all the way to the filesystem root.
            true
        }
    });
    candidates.sort_by_key(|path| {
        let file_match = reasons.contains_key(path);
        let (count, recency) = usage.get(path).copied().unwrap_or((0, usize::MAX));
        let likely = path.file_name().is_some_and(|name| {
            common_names.contains(&name.to_string_lossy().to_lowercase().as_str())
        });
        (
            std::cmp::Reverse(file_match),
            std::cmp::Reverse(count),
            recency,
            std::cmp::Reverse(likely),
            path.clone(),
        )
    });
    candidates
        .into_iter()
        .take(if query.is_empty() { 200 } else { 30 })
        .map(|path| {
            let reason = reasons
                .remove(&path)
                .unwrap_or_else(|| match usage.get(&path).copied() {
                    Some((count, _)) if count > 1 => format!("frequent · {count} sessions"),
                    Some(_) => "recent".into(),
                    None if path.file_name().is_some_and(|name| {
                        common_names.contains(&name.to_string_lossy().to_lowercase().as_str())
                    }) =>
                    {
                        "likely".into()
                    }
                    None => String::new(),
                });
            (path, reason)
        })
        .collect()
}
