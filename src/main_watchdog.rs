//! macOS main-thread liveness probe. It does not use GPUI or wait for a draw.

use dispatch2::DispatchQueue;
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INTERVAL: Duration = Duration::from_millis(500);
const HANG: Duration = Duration::from_secs(2);
const COOLDOWN: Duration = Duration::from_secs(60);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_REPORT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RAW_BYTES: u64 = 8 * 1024 * 1024;
const KEEP_REPORTS: usize = 8;
const REPORT_SUFFIX: &str = "-desktop-hang.txt";

pub fn start() -> io::Result<()> {
    let Some(home) = jcode_desktop_model::platform_dirs::jcode_home() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no Jcode home"));
    };
    let dir = home.join("logs/desktop-hangs");
    thread::Builder::new()
        .name("desktop-main-watchdog".into())
        .spawn(move || watch(&dir))?;
    Ok(())
}

fn watch(dir: &Path) {
    let mut last_capture = None::<Instant>;
    loop {
        let (ack, reply) = mpsc::channel();
        let sent = Instant::now();
        DispatchQueue::main().exec_async(move || {
            let _ = ack.send(());
        });
        if reply.recv_timeout(HANG) == Err(mpsc::RecvTimeoutError::Timeout) {
            if last_capture.is_none_or(|last| last.elapsed() >= COOLDOWN) {
                last_capture = Some(Instant::now());
                match capture(dir, sent.elapsed()) {
                    Ok(path) => log::warn!("Desktop main-thread stall report: {}", path.display()),
                    Err(error) => log::warn!("Could not save Desktop main-thread stall: {error}"),
                }
            }
            // Keep at most one outstanding probe, even if main never recovers.
            let _ = reply.recv();
        }
        thread::sleep(INTERVAL);
    }
}

fn capture(dir: &Path, lag: Duration) -> io::Result<PathBuf> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let pid = std::process::id();
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("{time:013}-{pid}{REPORT_SUFFIX}"));
    let raw = tempfile::NamedTempFile::new_in(dir)?;
    let result = sample(pid, raw.path());
    let mut report = tempfile::NamedTempFile::new_in(dir)?;
    writeln!(
        report,
        "Jcode Desktop {}: main queue did not respond for {}ms (pid {pid}, Unix ms {time})",
        jcode_desktop_ui::build_version(),
        lag.as_millis(),
    )?;
    if let Err(error) = &result {
        writeln!(report, "macOS sample did not complete: {error}")?;
    }
    let end = b"\n[Desktop watchdog report truncated]\n";
    let budget = MAX_REPORT_BYTES - report.as_file().metadata()?.len() - end.len() as u64;
    let raw = File::open(raw.path())?;
    let truncated = raw.metadata()?.len() > budget;
    io::copy(&mut raw.take(budget), &mut report)?;
    if truncated {
        report.write_all(end)?;
    }
    report.persist_noclobber(&path)?;
    prune(dir);
    Ok(path)
}

fn sample(pid: u32, path: &Path) -> io::Result<()> {
    // Keep pre_exec out of the Desktop process: it forces fork, whose malloc
    // locks can block the main thread we are trying to diagnose. The shell
    // sets its own file limit and replaces itself with sample.
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "ulimit -f \"$1\" || exit 1; shift; exec /usr/bin/sample \"$@\"",
            "watchdog-sample",
        ])
        .arg((MAX_RAW_BYTES / 1024).to_string())
        .args([pid.to_string().as_str(), "3", "10", "-file"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(status.to_string()))
            };
        }
        if started.elapsed() >= SAMPLE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "sample timed out"));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut reports: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().ends_with(REPORT_SUFFIX)
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
        .map(|entry| entry.path())
        .collect();
    reports.sort_unstable();
    let excess = reports.len().saturating_sub(KEEP_REPORTS);
    for path in reports.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

#[inline(never)]
pub fn block_main_for_test() {
    thread::sleep(Duration::from_secs(8));
    std::hint::black_box(());
}
