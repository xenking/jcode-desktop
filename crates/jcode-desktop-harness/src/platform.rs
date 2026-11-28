//! Small platform integration helpers shared by the desktop surfaces.

use std::path::{Path, PathBuf};

/// The jcode data directory shared with the CLI and the daemon.
pub fn jcode_home() -> Option<PathBuf> {
    jcode_desktop_model::platform_dirs::jcode_home()
}

/// Whether the offline screenshot harness is driving this process. Network,
/// updater and cloud side effects are suppressed so fixtures stay hermetic.
pub fn screenshot_mode() -> bool {
    std::env::var("JCODE_DESKTOP_SCREENSHOT").as_deref() == Ok("1")
}

/// Locate a companion executable shipped inside the app bundle, then fall back
/// to PATH for source builds. macOS GUI apps inherit a deliberately small PATH,
/// so depending on shell startup files would make Finder launches unreliable.
pub fn companion_executable(name: &str) -> PathBuf {
    #[cfg(windows)]
    let name = if name.to_ascii_lowercase().ends_with(".exe") {
        name.to_owned()
    } else {
        format!("{name}.exe")
    };
    #[cfg(not(windows))]
    let name = name.to_owned();
    if let Ok(current) = std::env::current_exe()
        && let Some(companion) = companion_next_to(&current, &name)
    {
        return companion;
    }
    PathBuf::from(name)
}

/// Keep a console-subsystem child (the `jcode` CLI, `ssh`, `python`) from
/// opening a console window. Jcode Desktop is a GUI-subsystem app on
/// Windows, so it has no console for children to share, and each one would
/// otherwise flash a black terminal window. No-op elsewhere.
pub fn no_console_window(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The desktop executable to relaunch for new windows. A long-running host
/// whose binary was rebuilt in place reports `/path (deleted)` on Linux, which
/// cannot be spawned. The rebuilt file at the original path is the right
/// target, so strip that marker whenever the unmarked path still exists.
pub fn self_executable() -> std::io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    Ok(relaunchable(current))
}

fn relaunchable(current: PathBuf) -> PathBuf {
    if current.exists() {
        return current;
    }
    current
        .to_str()
        .and_then(|path| path.strip_suffix(" (deleted)"))
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or(current)
}

fn companion_next_to(current: &Path, name: &str) -> Option<PathBuf> {
    let sibling = current.parent()?.join(name);
    is_executable(&sibling).then_some(sibling)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relaunches_the_rebuilt_binary_after_an_in_place_rebuild() {
        let root = std::env::temp_dir().join(format!("jcode-relaunch-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("jcode-desktop");
        fs::write(&binary, []).unwrap();
        let deleted = PathBuf::from(format!("{} (deleted)", binary.display()));
        assert_eq!(relaunchable(deleted), binary);
        assert_eq!(relaunchable(binary.clone()), binary);
        let missing = root.join("gone (deleted)");
        assert_eq!(relaunchable(missing.clone()), missing);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_companion_falls_back_to_the_program_name() {
        let name = "jcode-companion-that-does-not-exist";
        assert_eq!(companion_executable(name), PathBuf::from(name));
    }

    #[cfg(unix)]
    #[test]
    fn finds_an_executable_bundle_companion_without_using_path() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("jcode-bundle-test-{}", std::process::id()));
        let macos = root.join("Jcode.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        let desktop = macos.join("jcode-desktop");
        let cli = macos.join("jcode");
        fs::write(&desktop, []).unwrap();
        fs::write(&cli, []).unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(companion_next_to(&desktop, "jcode"), Some(cli));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ignores_a_non_executable_bundle_companion() {
        let root =
            std::env::temp_dir().join(format!("jcode-bundle-mode-test-{}", std::process::id()));
        let macos = root.join("Jcode.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        let cli = macos.join("jcode");
        fs::write(&cli, []).unwrap();

        assert_eq!(
            companion_next_to(&macos.join("jcode-desktop"), "jcode"),
            None
        );
        fs::remove_dir_all(root).unwrap();
    }
}

/// Whether something is actually listening on a Unix socket path. A socket
/// file left behind by a dead process still exists, so `Path::exists` (which
/// the SDK uses) reports it as healthy and nothing is ever respawned.
#[cfg(unix)]
pub fn socket_live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
pub fn socket_live(path: &Path) -> bool {
    path.exists()
}

/// Remove a socket file nobody listens on, so a respawn is not short-circuited
/// by the stale path. Returns whether a stale file was removed.
pub fn clear_stale_socket(path: &Path) -> bool {
    if cfg!(unix) && path.exists() && !socket_live(path) {
        return std::fs::remove_file(path).is_ok();
    }
    false
}

/// Pick the runtime directory whose daemon socket is live when the inherited
/// one is not. An agent shell may export `TMPDIR` to a scratch directory that
/// holds a stale `jcode.sock`; the shared daemon lives in the per-user Darwin
/// temp dir. Returns `None` when the default resolution is already right or
/// nothing better is live.
pub fn better_runtime_dir(
    current: &Path,
    candidates: &[PathBuf],
    live: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if live(&current.join("jcode.sock")) {
        return None;
    }
    candidates
        .iter()
        .find(|dir| dir.as_path() != current && live(&dir.join("jcode.sock")))
        .cloned()
}

/// The per-user Darwin temp directory (`getconf DARWIN_USER_TEMP_DIR`), which
/// is the `TMPDIR` launchd gives GUI apps and where the shared daemon listens.
#[cfg(target_os = "macos")]
pub fn darwin_user_temp_dir() -> Option<PathBuf> {
    let output = std::process::Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim().trim_end_matches('/');
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// Align `JCODE_RUNTIME_DIR` with the live shared daemon before any thread
/// starts. Explicit `JCODE_RUNTIME_DIR`/`XDG_RUNTIME_DIR` choices are kept.
///
/// # Safety
/// Mutates the process environment, so call before other threads exist.
pub unsafe fn normalize_runtime_dir() {
    #[cfg(target_os = "macos")]
    {
        if std::env::var_os("JCODE_RUNTIME_DIR").is_some()
            || std::env::var_os("XDG_RUNTIME_DIR").is_some()
        {
            return;
        }
        let current = jcode_sdk::api::runtime_dir();
        let mut candidates = Vec::new();
        if let Some(socket) = std::env::var_os("JCODE_SOCKET")
            && let Some(parent) = Path::new(&socket).parent()
        {
            candidates.push(parent.to_path_buf());
        }
        candidates.extend(darwin_user_temp_dir());
        if let Some(dir) = better_runtime_dir(&current, &candidates, socket_live) {
            eprintln!(
                "jcode desktop: runtime dir {} has no live daemon, using {}",
                current.display(),
                dir.display()
            );
            // SAFETY: the caller guarantees no other thread exists yet.
            unsafe {
                std::env::set_var("JCODE_RUNTIME_DIR", &dir);
                std::env::set_var("JCODE_SOCKET", dir.join("jcode.sock"));
            }
        }
    }
}

#[cfg(all(test, unix))]
mod socket_tests {
    use super::*;
    use std::os::unix::net::{UnixDatagram, UnixListener};

    /// A socket file that refuses stream connections. A dropped `UnixListener`
    /// is not deterministic: a child forked by a concurrent test keeps a copy of
    /// the descriptor until it execs, so the "dead" listener still accepts. A
    /// bound datagram socket cannot be revived that way.
    fn stale_socket(path: &Path) {
        drop(UnixDatagram::bind(path).unwrap());
    }

    #[test]
    fn a_stale_socket_file_is_not_live_and_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jcode-api.sock");
        stale_socket(&path);
        assert!(path.exists(), "the dead listener leaves its file behind");
        assert!(!socket_live(&path));
        assert!(clear_stale_socket(&path));
        assert!(!path.exists());
    }

    #[test]
    fn a_listening_socket_is_live_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jcode-api.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        assert!(socket_live(&path));
        assert!(!clear_stale_socket(&path));
        assert!(path.exists());
    }

    #[test]
    fn tmpdir_with_a_stale_daemon_socket_falls_back_to_the_live_one() {
        let scratch = tempfile::tempdir().unwrap();
        stale_socket(&scratch.path().join("jcode.sock"));
        let darwin = tempfile::tempdir().unwrap();
        let _daemon = UnixListener::bind(darwin.path().join("jcode.sock")).unwrap();
        let candidates = vec![darwin.path().to_path_buf()];
        assert_eq!(
            better_runtime_dir(scratch.path(), &candidates, socket_live),
            Some(darwin.path().to_path_buf())
        );
        // A healthy inherited dir is left alone.
        assert_eq!(
            better_runtime_dir(darwin.path(), &candidates, socket_live),
            None
        );
    }
}
