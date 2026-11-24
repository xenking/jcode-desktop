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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    WhenIdle,
    Now,
}

fn apply_args(preview: &Preview, install: Install) -> Vec<&str> {
    let mut args = vec![
        "--apply-reviewed",
        "--jcode-ref",
        &preview.cli.target_sha,
        "--desktop-ref",
        &preview.desktop.target_sha,
    ];
    if install == Install::Now {
        args.push("--now");
    }
    args
}

#[derive(Deserialize)]
pub struct Receipt {
    id: String,
}

pub fn submit(preview: &Preview, install: Install) -> Result<Receipt> {
    let output = command(&cli_path()?)?
        .args(apply_args(preview, install))
        .output()?;
    let raw = checked(
        output,
        "Couldn't prepare the reviewed update. Check available disk space, signing, and whether another rollout is running",
    )?;
    let raw = raw.trim_ascii();
    let receipt = raw.rsplit(|b| *b == b'\n').next().unwrap_or_default();
    serde_json::from_slice(receipt).context("The bundled CLI did not confirm the submitted update")
}

#[derive(Deserialize)]
struct Status {
    id: String,
    phase: String,
    intent: Option<Intent>,
    error: Option<String>,
    #[serde(default)]
    blockers: Vec<Blocker>,
    retry_blockers: Option<Vec<Blocker>>,
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
    Draining(Vec<String>),
    Completed,
    Blocked {
        sessions: Vec<String>,
        retry_blockers: Option<Vec<String>>,
    },
    Failed,
}

#[derive(Deserialize)]
struct Blocker {
    session_id: String,
    phase: String,
    reason: String,
}

impl Blocker {
    fn describe(self) -> String {
        let name = self
            .session_id
            .strip_prefix("session_")
            .and_then(|id| id.split_once('_').map(|(name, _)| name))
            .filter(|name| {
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
            });
        let name = name.unwrap_or(&self.session_id);
        let reason = match (self.phase.as_str(), self.reason.as_str()) {
            ("ask_blocked", _) | (_, "waiting for user answer, checkpointable on interruption") => {
                "is waiting for your answer"
            }
            ("in_tool", "executing tool bash") | ("", "executing tool bash") => {
                "is running a shell command"
            }
            ("in_tool", _) | ("", _) if self.reason.starts_with("executing tool ") => {
                "is running a tool"
            }
            ("checkpoint_requested", _) => "is saving its turn",
            ("rollout_waiting", _) => "is waiting for another update",
            _ => "has an active turn",
        };
        format!("{name} {reason}")
    }
}

fn legacy_blockers(error: Option<&str>) -> Vec<Blocker> {
    let Some(detail) = error.and_then(|s| s.strip_prefix("drain timed out: ")) else {
        return Vec::new();
    };
    detail
        .split("), ")
        .filter_map(|entry| {
            let (id, reason) = entry.split_once(" (")?;
            if !id.starts_with("session_")
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            Some(Blocker {
                session_id: id.into(),
                phase: String::new(),
                reason: reason.trim_end_matches(')').into(),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedAction {
    Cancel,
    RetryWhenIdle,
    InstallNow,
}

impl BlockedAction {
    pub const ALL: [Self; 3] = [Self::Cancel, Self::InstallNow, Self::RetryWhenIdle];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::RetryWhenIdle => "Retry when idle",
            Self::InstallNow => "Install now",
        }
    }
}

pub fn progress(preview: &Preview, receipt: &Receipt) -> Result<Progress> {
    let raw = probe(
        command(&cli_path()?)?.arg("--status"),
        Duration::from_secs(35),
        "Couldn't read rollout status",
    )?;
    progress_from_status(preview, receipt, &raw)
}

fn progress_from_status(preview: &Preview, receipt: &Receipt, raw: &[u8]) -> Result<Progress> {
    let status: Status = serde_json::from_slice(raw)?;
    let intent = status.intent.context("No submitted update was found")?;
    if status.id != receipt.id
        || !intent.reviewed
        || intent.cli_sha.as_deref() != Some(&preview.cli.target_sha)
        || intent.desktop != preview.desktop.target_sha
    {
        bail!("Another update has replaced this one. Check for updates again.");
    }
    Ok(match status.phase.as_str() {
        "completed" => Progress::Completed,
        "rolled_back" | "failed" => {
            let blockers = if status.blockers.is_empty() {
                legacy_blockers(status.error.as_deref())
            } else {
                status.blockers
            };
            if blockers.is_empty() {
                Progress::Failed
            } else {
                Progress::Blocked {
                    sessions: blockers.into_iter().map(Blocker::describe).collect(),
                    retry_blockers: status
                        .retry_blockers
                        .map(|items| items.into_iter().map(Blocker::describe).collect()),
                }
            }
        }
        "draining" => {
            Progress::Draining(status.blockers.into_iter().map(Blocker::describe).collect())
        }
        "exec_pending" | "serving" if !status.blockers.is_empty() => {
            Progress::Draining(status.blockers.into_iter().map(Blocker::describe).collect())
        }
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
        let args = apply_args(&preview, Install::WhenIdle);
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
        let force = apply_args(&preview, Install::Now);
        assert_eq!(force.split_last(), Some((&"--now", args.as_slice())));
    }

    #[test]
    fn drain_status_keeps_session_reasons_and_offers_recovery_actions() {
        let preview = preview();
        let receipt = Receipt {
            id: "rollout_1".into(),
        };
        let mut status = serde_json::json!({
            "id": receipt.id,
            "phase": "draining",
            "intent": {"cli_sha":preview.cli.target_sha,"desktop":preview.desktop.target_sha,"reviewed":true},
            "blockers": [
                {"session_id":"session_raccoon_1","phase":"ask_blocked","reason":"waiting for user answer, checkpointable on interruption"},
                {"session_id":"session_turkey_2","phase":"admitted","reason":"turn owns admission or publication lease"}
            ]
        });
        let progress = |status: &serde_json::Value| {
            progress_from_status(&preview, &receipt, &serde_json::to_vec(status).unwrap())
        };
        let reasons = vec![
            "raccoon is waiting for your answer".into(),
            "turkey has an active turn".into(),
        ];
        assert_eq!(
            progress(&status).unwrap(),
            Progress::Draining(reasons.clone())
        );
        status["phase"] = "rolled_back".into();
        status["retry_blockers"] = serde_json::json!([]);
        assert_eq!(
            progress(&status).unwrap(),
            Progress::Blocked {
                sessions: reasons,
                retry_blockers: Some(vec![])
            }
        );
        assert_eq!(
            BlockedAction::ALL.map(BlockedAction::label),
            ["Cancel", "Install now", "Retry when idle"]
        );
        status.as_object_mut().unwrap().remove("blockers");
        status.as_object_mut().unwrap().remove("retry_blockers");
        status["error"] =
            "drain timed out: session_turkey_2 (turn owns admission or publication lease)".into();
        assert_eq!(
            progress(&status).unwrap(),
            Progress::Blocked {
                sessions: vec!["turkey has an active turn".into()],
                retry_blockers: None,
            }
        );
        status["id"] = "rollout_2".into();
        assert!(
            progress(&status).is_err(),
            "same refs are not proof of the same attempt"
        );
    }
}
