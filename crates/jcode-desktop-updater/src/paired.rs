//! Desktop's subprocess contract with the bundled `jcode rollout` command.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
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
        .env("JCODE_BUILD_RECLAIM", "")
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

fn probe(command: &mut Command, timeout: Duration, message: &str) -> Result<Vec<u8>> {
    // Use a file instead of a pipe so a helper spawned by git cannot keep the
    // updater blocked on EOF after the CLI has exited.
    let mut stdout = tempfile::tempfile()?;
    let mut child = command
        .stdout(stdout.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{message} (timed out)");
        }
        thread::sleep(Duration::from_millis(25).min(remaining));
    };
    if !status.success() {
        bail!("{message} ({status})");
    }
    let mut bytes = Vec::new();
    stdout.seek(SeekFrom::Start(0))?;
    stdout.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn check() -> Result<Option<Preview>> {
    static CHECK: Mutex<()> = Mutex::new(());
    let _check = CHECK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = probe(
        command(&cli_path()?)?.arg("--check-update"),
        Duration::from_secs(90),
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

#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    InProgress,
    Completed,
    Failed,
}

pub fn progress(preview: &Preview) -> Result<Progress> {
    let raw = probe(
        command(&cli_path()?)?.arg("--status"),
        Duration::from_secs(35),
        "Couldn't read rollout status",
    )?;
    progress_from_status(preview, &raw)
}

fn progress_from_status(preview: &Preview, raw: &[u8]) -> Result<Progress> {
    let status: Status = serde_json::from_slice(raw)?;
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

    #[cfg(unix)]
    #[test]
    fn read_only_probes_return_output_or_timeout() {
        let result = probe(
            Command::new("/bin/sh").args(["-c", "printf null; printf hidden >&2"]),
            Duration::from_secs(5),
            "read preview",
        );
        assert_eq!(result.unwrap(), b"null");
        let result = probe(
            Command::new("/bin/sleep").arg("30"),
            Duration::ZERO,
            "read preview",
        );
        assert_eq!(result.unwrap_err().to_string(), "read preview (timed out)");
    }

    #[test]
    fn update_reports_low_disk_without_reclaiming_caches() {
        let command = command(Path::new("jcode")).unwrap();
        assert!(command.get_envs().any(|(key, value)| {
            key == "JCODE_BUILD_RECLAIM" && value.is_some_and(|value| value.is_empty())
        }));
    }

    fn preview() -> Preview {
        let change = |sha: &str| Change {
            installed_version: "v1.0.0".into(),
            installed_sha: "1".repeat(40),
            target_version: "1.0.0".into(),
            target_sha: sha.repeat(40),
            commits: 1,
            subjects: vec!["Fix a bug".into()],
        };
        Preview {
            cli: change("a"),
            desktop: change("b"),
            free_gib: 60,
            minimum_gib: 30,
        }
    }

    #[test]
    fn reviewed_subprocess_uses_only_the_confirmed_refs_without_force() {
        let preview = preview();
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

    #[test]
    fn drain_status_keeps_session_reasons_and_offers_recovery_actions() {
        let preview = preview();
        let mut status = serde_json::json!({
            "phase": "draining",
            "intent": {"cli_sha":preview.cli.target_sha,"desktop":preview.desktop.target_sha,"reviewed":true},
            "blockers": [
                {"session_id":"session_raccoon_1","phase":"ask_blocked","reason":"waiting for user answer, checkpointable on interruption"},
                {"session_id":"session_turkey_2","phase":"admitted","reason":"turn owns admission or publication lease"}
            ]
        });
        let progress = |status: &serde_json::Value| {
            format!("{:?}", progress_from_status(&preview, &serde_json::to_vec(status).unwrap()).unwrap())
        };
        let draining = progress(&status);
        assert!(draining.contains("raccoon is waiting for your answer"), "{draining}");
        assert!(draining.contains("turkey has an active turn"), "{draining}");
        status["phase"] = "rolled_back".into();
        let failed = progress(&status);
        for text in ["turkey has an active turn", "Install now", "Retry when idle", "Cancel"] {
            assert!(failed.contains(text), "Missing {text}: {failed}");
        }
    }
}
