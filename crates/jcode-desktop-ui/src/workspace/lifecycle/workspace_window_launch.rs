//! Spawning separate native Desktop windows: new sessions and forks.
//!
//! Both open in their own single-panel process, so nothing here touches the
//! workspace canvas or any other workspace module.
use super::*;

/// Launch arguments for a forked session's own single-panel window. The shared
/// single-panel host receives this launch and opens the fork beside this one.
pub(super) fn fork_window_args(
    session_id: &str,
    own: impl Iterator<Item = std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--single-panel".into(),
        format!("--session={session_id}").into(),
    ];
    args.extend(own.filter(|arg| arg == "--hot-reload" || arg == "--no-hot-reload"));
    args
}

#[cfg(test)]
thread_local! {
    pub(super) static NEW_SESSION_WINDOWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static FORK_WINDOWS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Workspace {
    pub(super) fn open_new_session_window(&mut self, _cx: &mut Context<Self>) {
        #[cfg(test)]
        NEW_SESSION_WINDOWS.with(|count| count.set(count.get() + 1));
        #[cfg(not(test))]
        {
            if harness::screenshot_mode() {
                return;
            }
            let result = crate::platform::self_executable().and_then(|executable| {
                let mut command = std::process::Command::new(executable);
                command.arg("--single-panel");
                command.args(
                    std::env::args_os()
                        .filter(|arg| arg == "--hot-reload" || arg == "--no-hot-reload"),
                );
                if let Some(directory) = self.new_session_directory()
                    && std::path::Path::new(&directory).is_dir()
                {
                    command.env("JCODE_DESKTOP_WORKING_DIR", &directory);
                    command.current_dir(directory);
                }
                command.spawn().map(|_| ())
            });
            if let Err(error) = result {
                eprintln!("new session: could not open window: {error}");
                self.status = format!("Could not open a new session window: {error}");
                _cx.notify();
            }
        }
    }

    /// A fork never replaces this window's chat. It opens in a new window.
    pub(super) fn open_fork_window(&self, session_id: &str) {
        #[cfg(test)]
        {
            FORK_WINDOWS.with(|log| log.borrow_mut().push(session_id.to_owned()));
        }
        #[cfg(not(test))]
        {
            if harness::screenshot_mode() {
                return;
            }
            let Ok(executable) = crate::platform::self_executable() else {
                eprintln!("fork: cannot locate the desktop executable");
                return;
            };
            let mut command = std::process::Command::new(executable);
            command.args(fork_window_args(session_id, std::env::args_os().skip(1)));
            if let Err(error) = command.spawn() {
                eprintln!("fork: could not open forked window: {error}");
            }
        }
    }
}
