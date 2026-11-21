//! Per-window launch requests for the shared single-panel host.
//!
//! One host process can own several single-panel windows. Each window keeps
//! the arguments and selected environment of the launch that asked for it, so
//! `--resume`, `--session=<id>` and `JCODE_DESKTOP_WORKING_DIR` stay scoped to
//! that window instead of the process that happened to start first.
//!
//! This Global lives in the shared API crate for the same reason as
//! [`crate::ImageIds`]: a UI-local Global would get a different TypeId in the
//! hot-reloaded cdylib and silently split host and UI state.
use std::{collections::HashMap, ffi::OsString, rc::Rc};

use gpui::{App, Global};

/// Environment forwarded from a launching process to its window. Only
/// window-scoped settings belong here, never credentials or process config.
pub const FORWARDED_ENV: &[&str] = &["JCODE_DESKTOP_WORKING_DIR", "JCODE_DESKTOP_STATE"];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WindowLaunch {
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

impl WindowLaunch {
    /// The launch that started this process.
    pub fn current_process() -> Self {
        Self::from_parts(
            std::env::args_os().skip(1),
            FORWARDED_ENV
                .iter()
                .filter_map(|key| Some((key.to_string(), std::env::var(key).ok()?))),
        )
    }

    pub fn from_parts(
        args: impl IntoIterator<Item = impl Into<OsString>>,
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            env: env
                .into_iter()
                .filter(|(key, _)| FORWARDED_ENV.contains(&key.as_str()))
                .collect(),
        }
    }

    pub fn var(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Compact wire form: NUL-separated `a:<arg>` and `e:<KEY>=<value>`.
    /// Non-UTF-8 arguments are converted lossily, matching the UI's own use.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for arg in &self.args {
            out.extend_from_slice(b"a:");
            out.extend_from_slice(arg.to_string_lossy().as_bytes());
            out.push(0);
        }
        for (key, value) in &self.env {
            out.extend_from_slice(b"e:");
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
            out.extend_from_slice(value.as_bytes());
            out.push(0);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut args = Vec::new();
        let mut env = Vec::new();
        for entry in text.split('\0').filter(|entry| !entry.is_empty()) {
            if let Some(arg) = entry.strip_prefix("a:") {
                args.push(OsString::from(arg));
            } else if let Some((key, value)) = entry.strip_prefix("e:")?.split_once('=') {
                env.push((key.to_string(), value.to_string()));
            } else {
                return None;
            }
        }
        Some(Self::from_parts(args, env))
    }
}

type BackgroundWindowOpener = Rc<dyn Fn(WindowLaunch, &mut App) -> Result<u64, String>>;

/// Window-scoped launches and their host's native window factory.
#[derive(Default)]
pub struct WindowLaunches {
    launches: HashMap<u64, WindowLaunch>,
    shared: bool,
    background_opener: Option<BackgroundWindowOpener>,
}

impl Global for WindowLaunches {}

impl WindowLaunches {
    /// Mark this App as a shared single-panel host.
    pub fn install(cx: &mut App) {
        cx.default_global::<Self>().shared = true;
    }

    pub fn shared(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|state| state.shared)
    }

    /// Forwarded launches are separate, intentionally fresh windows.
    pub fn is_forwarded(cx: &App, window: u64) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|state| state.launches.contains_key(&window))
    }

    pub fn insert(cx: &mut App, window: u64, launch: WindowLaunch) {
        cx.default_global::<Self>().launches.insert(window, launch);
    }

    pub fn remove(cx: &mut App, window: u64) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>().launches.remove(&window);
        }
    }

    /// The window's own launch, or the process launch for windows the host
    /// did not register (the first window, or an older host).
    pub fn get(cx: &App, window: u64) -> WindowLaunch {
        cx.try_global::<Self>()
            .and_then(|state| state.launches.get(&window).cloned())
            .unwrap_or_else(WindowLaunch::current_process)
    }

    pub fn set_background_opener(
        cx: &mut App,
        open: impl Fn(WindowLaunch, &mut App) -> Result<u64, String> + 'static,
    ) {
        cx.default_global::<Self>().background_opener = Some(Rc::new(open));
    }

    pub fn open_in_background(cx: &mut App, launch: WindowLaunch) -> Result<u64, String> {
        let open = cx
            .try_global::<Self>()
            .and_then(|state| state.background_opener.clone())
            .ok_or_else(|| "host does not support opening a window".to_owned())?;
        open(launch, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_form_round_trips_and_drops_unforwarded_environment() {
        let launch = WindowLaunch::from_parts(
            ["--single-panel", "--session=abc", "has space"],
            [
                ("JCODE_DESKTOP_WORKING_DIR".into(), "/tmp/a=b".into()),
                ("SECRET_TOKEN".into(), "nope".into()),
            ],
        );
        assert_eq!(launch.var("SECRET_TOKEN"), None);
        let decoded = WindowLaunch::decode(&launch.encode()).unwrap();
        assert_eq!(decoded, launch);
        assert_eq!(decoded.var("JCODE_DESKTOP_WORKING_DIR"), Some("/tmp/a=b"));
        assert_eq!(WindowLaunch::decode(b"x:bad\0"), None);
        assert_eq!(WindowLaunch::decode(b""), Some(WindowLaunch::default()));
    }
}
