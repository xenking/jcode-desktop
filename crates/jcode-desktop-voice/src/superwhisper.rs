//! The managed SuperWhisper bridge: a script installed next to the jcode
//! config that `[dictation] command` points at. Desktop recognizes that exact
//! command to know the bridge owns the microphone and can be told to stop.

use std::path::{Path, PathBuf};

/// Write `script` as the bridge next to the jcode config and return the
/// command line to store in `[dictation] command`.
pub fn install(script: &str) -> std::io::Result<String> {
    let path = bridge_path().ok_or_else(|| std::io::Error::other("no jcode home"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(command_for(&path))
}

/// Where the bridge script lives.
fn bridge_path() -> Option<PathBuf> {
    Some(jcode_desktop_model::platform_dirs::jcode_home()?.join("bin/superwhisper-dictate.sh"))
}

/// The shell command line that runs the script at `path`.
fn command_for(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Whether `command` is the managed bridge, which only exists on macOS.
pub fn manages(command: &str) -> bool {
    cfg!(target_os = "macos") && bridge_path().is_some_and(|path| command == command_for(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_quote_paths_with_single_quotes() {
        assert_eq!(command_for(Path::new("/a b/x.sh")), "'/a b/x.sh'");
        assert_eq!(command_for(Path::new("/it's/x.sh")), "'/it'\\''s/x.sh'");
    }
}
