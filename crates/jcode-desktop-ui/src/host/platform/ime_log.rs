//! Opt-in IME diagnostics. Log identities and state only, never prompt text.
#[cfg(target_os = "macos")]
pub(crate) fn enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("JCODE_DESKTOP_IME_LOG").as_deref() == Some(std::ffi::OsStr::new("1"))
    })
}

pub(crate) fn record(event: impl std::fmt::Display) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt, sync::OnceLock};
    static FILE: OnceLock<Option<std::fs::File>> = OnceLock::new();
    if !enabled() {
        return;
    }
    let file = FILE.get_or_init(|| {
        let home = std::env::var_os("HOME")?;
        let path = std::path::PathBuf::from(home).join(".jcode/logs/input-source.log");
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(path)
            .ok()
    });
    if let Some(mut file) = file.as_ref() {
        let line = format!(
            "{} pid={} {}\n",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            std::process::id(),
            event
        );
        let _ = file.write_all(line.as_bytes());
    }
}
