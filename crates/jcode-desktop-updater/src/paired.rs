//! Desktop's subprocess contract with the bundled `jcode rollout` command.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Mutex,
};

#[derive(Debug, Deserialize)]
pub struct Change {
    pub installed_version: String,
    pub installed_sha: String,
    pub target_version: String,
    pub target_sha: String,
    pub commits: usize,
    pub subjects: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Preview {
    pub cli: Change,
    pub desktop: Change,
    pub free_gib: u64,
    pub minimum_gib: u64,
}

pub fn configured() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && option_env!("JCODE_DESKTOP_PAIRED_ROLLOUT") == Some("1")
}

fn cli_path() -> Result<PathBuf> {
    let app = std::env::var_os("JCODE_ROLLOUT_APP")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Applications/Jcode.app"));
    let bin = app.join("Contents/MacOS");
    if !configured()
        || std::env::current_exe()?.canonicalize()? != bin.join("jcode-desktop").canonicalize()?
    {
        bail!("Run this update check from the installed Jcode.app.");
    }
    Ok(bin.join("jcode"))
}

fn command(cli: &Path) -> Result<Command> {
    // Finder's PATH generally lacks cargo, rustup, and Homebrew.
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.extend([home.join(".cargo/bin"), home.join(".local/bin")]);
    }
    paths.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    paths.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .map(PathBuf::from),
    );
    let mut command = Command::new(cli);
    command
        .args(["--no-update", "rollout"])
        .env("PATH", std::env::join_paths(paths)?)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("JCODE_SESSION_ID")
        .stdin(Stdio::null());
    Ok(command)
}

fn checked(output: Output, message: &str) -> Result<Vec<u8>> {
    // Do not surface raw build/Git output in dialogs: it may contain secrets
    // entered in a remote URL or emitted by a local build tool.
    if !output.status.success() {
        bail!("{message} ({})", output.status);
    }
    Ok(output.stdout)
}

pub fn check() -> Result<Option<Preview>> {
    static CHECK: Mutex<()> = Mutex::new(());
    let _check = CHECK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let output = command(&cli_path()?)?.arg("--check-update").output()?;
    let result = checked(
        output,
        "Couldn't check the fork heads. Verify the local rollout checkouts and Git authentication",
    )?;
    serde_json::from_slice(&result).context("The bundled CLI returned an invalid update preview")
}

fn apply_args(preview: &Preview) -> [&str; 5] {
    [
        "--apply-reviewed",
        "--jcode-ref",
        &preview.cli.target_sha,
        "--desktop-ref",
        &preview.desktop.target_sha,
    ]
}

pub fn submit(preview: &Preview) -> Result<()> {
    let output = command(&cli_path()?)?.args(apply_args(preview)).output()?;
    checked(
        output,
        "Couldn't prepare the reviewed update. Check available disk space, signing, and whether another rollout is running",
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct Status {
    phase: String,
    intent: Option<Intent>,
}

#[derive(Deserialize)]
struct Intent {
    cli_sha: Option<String>,
    desktop: String,
    #[serde(default)]
    reviewed: bool,
}

#[derive(PartialEq, Eq)]
pub enum Progress {
    InProgress,
    Completed,
    Failed,
}

pub fn progress(preview: &Preview) -> Result<Progress> {
    let output = command(&cli_path()?)?.arg("--status").output()?;
    let raw = checked(output, "Couldn't read rollout status")?;
    let status: Status = serde_json::from_slice(&raw)?;
    let intent = status.intent.context("No submitted update was found")?;
    if !intent.reviewed
        || intent.cli_sha.as_deref() != Some(&preview.cli.target_sha)
        || intent.desktop != preview.desktop.target_sha
    {
        bail!("Another rollout owns the current status. See jcode rollout --status.");
    }
    Ok(match status.phase.as_str() {
        "completed" => Progress::Completed,
        "rolled_back" | "failed" => Progress::Failed,
        _ => Progress::InProgress,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_subprocess_uses_only_the_confirmed_refs_without_force() {
        let change = |sha: &str| Change {
            installed_version: "v1.0.0".into(),
            installed_sha: "1".repeat(40),
            target_version: "1.0.0".into(),
            target_sha: sha.repeat(40),
            commits: 1,
            subjects: vec!["Fix a bug".into()],
        };
        let preview = Preview {
            cli: change("a"),
            desktop: change("b"),
            free_gib: 60,
            minimum_gib: 30,
        };
        let args = apply_args(&preview);
        assert_eq!(
            args,
            [
                "--apply-reviewed",
                "--jcode-ref",
                &"a".repeat(40),
                "--desktop-ref",
                &"b".repeat(40)
            ]
        );
    }
}
