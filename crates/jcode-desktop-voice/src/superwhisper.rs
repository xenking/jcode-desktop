//! The managed SuperWhisper bridge: a script installed next to the jcode
//! config that `[dictation] command` points at. Desktop recognizes that exact
//! command to know the bridge owns the microphone and can be told to stop.

use std::path::{Path, PathBuf};

/// Where the bridge script lives.
pub fn bridge_path() -> Option<PathBuf> {
    Some(jcode_desktop_model::platform_dirs::jcode_home()?.join("bin/superwhisper-dictate.sh"))
}

/// The shell command line that runs the script at `path`.
pub fn command_for(path: &Path) -> String {
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
