//! Local, per-window Desktop control transport. Requests are executed on GPUI's
//! window thread, never on this socket thread.
#[cfg(unix)]
mod unix {
    use jcode_sdk::desktop_control::{
        COMPUTER_USE_TIMEOUT_SECS, DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest,
        DesktopResponse,
    };
    use std::{
        io::{self, Read, Write},
        net::Shutdown,
        os::unix::{
            fs::{DirBuilderExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    pub struct Pending {
        pub request: DesktopRequest,
        pub reply: mpsc::SyncSender<DesktopResponse>,
        pub deadline: Instant,
        pub cancelled: Arc<AtomicBool>,
    }

    pub struct Server {
        path: PathBuf,
        identity: (u64, u64),
        stop: Arc<AtomicBool>,
        stop_accept: UnixStream,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Server {
        pub fn start(window_id: u64) -> io::Result<(Self, async_channel::Receiver<Pending>)> {
            Self::start_in(jcode_sdk::desktop_control::socket_dir(), window_id)
        }

        pub fn start_in(
            base: PathBuf,
            window_id: u64,
        ) -> io::Result<(Self, async_channel::Receiver<Pending>)> {
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&base) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
            let meta = std::fs::symlink_metadata(&base)?;
            if !meta.is_dir()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o077 != 0
            {
                return Err(io::Error::other(
                    "desktop control directory must be owned by this user and private",
                ));
            }
            // A new name on each UI generation avoids unlinking an older live endpoint.
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = base.join(format!("{}-{window_id}-{nonce}.sock", std::process::id()));
            let listener = UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            let meta = std::fs::symlink_metadata(&path)?;
            listener.set_nonblocking(true)?;
            let (stop_accept, closing) = UnixStream::pair()?;
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let waiters = Arc::new(AtomicUsize::new(0));
            let (tx, rx) = async_channel::bounded(8);
            let thread = thread::spawn(move || {
                while !worker_stop.load(Ordering::Acquire)
                    && crate::unix_listener_ready::wait(&listener, &closing).unwrap_or(false)
                {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if let Err(e) = handle(&mut stream, &tx, &worker_stop, &waiters) {
                                let response = DesktopResponse::Error {
                                    code: "transport".into(),
                                    message: e.to_string(),
                                };
                                let _ = writeln!(
                                    stream,
                                    "{}",
                                    serde_json::to_string(&response).unwrap_or_default()
                                );
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                            ) =>
                        {
                            continue;
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok((
                Self {
                    path,
                    identity: (meta.dev(), meta.ino()),
                    stop,
                    stop_accept,
                    thread: Some(thread),
                },
                rx,
            ))
        }
        #[cfg(test)]
        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    fn handle(
        stream: &mut UnixStream,
        tx: &async_channel::Sender<Pending>,
        stop: &Arc<AtomicBool>,
        waiters: &Arc<AtomicUsize>,
    ) -> io::Result<()> {
        // Accepted sockets can inherit the listener's nonblocking mode on macOS.
        // A large semantic state must not be truncated after its first buffer.
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(io::Error::other("request timed out"));
            }
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => return Err(io::Error::other("request must end with newline")),
                Ok(_) if byte[0] == b'\n' => break,
                Ok(_) => {
                    bytes.push(byte[0]);
                    if bytes.len() > 1024 * 1024 {
                        return Err(io::Error::other("request too large"));
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(e),
            }
        }
        let request: DesktopRequest = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if matches!(request.action, DesktopAction::ComputerUse { .. }) {
            if request.v != DESKTOP_CONTROL_VERSION {
                return Err(io::Error::other("unsupported Desktop control version"));
            }
            let mut socket = stream.try_clone()?;
            if !reserve(waiters) {
                return Err(io::Error::other("too many concurrent desktop requests"));
            }
            let tx = tx.clone();
            let stop = stop.clone();
            let waiters = waiters.clone();
            thread::spawn(move || {
                let _ = computer(&mut socket, request, &tx, &stop);
                waiters.fetch_sub(1, Ordering::AcqRel);
            });
            return Ok(());
        }
        if let DesktopAction::WaitFor {
            target,
            condition,
            timeout_ms,
        } = request.action
        {
            if request.v != DESKTOP_CONTROL_VERSION || !(1..=120_000).contains(&timeout_ms) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid wait version or timeout",
                ));
            }
            if let jcode_sdk::desktop_control::WaitCondition::SelectorVisible { selector } =
                &condition
                && !selector.starts_with("tool:")
                && !selector.starts_with("row:")
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only transcript selector visibility is supported",
                ));
            }
            if !reserve(waiters) {
                return Err(io::Error::other("too many concurrent desktop waits"));
            }
            let mut socket = match stream.try_clone() {
                Ok(socket) => socket,
                Err(error) => {
                    waiters.fetch_sub(1, Ordering::AcqRel);
                    return Err(error);
                }
            };
            let tx = tx.clone();
            let stop = stop.clone();
            let waiters = waiters.clone();
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                let response = loop {
                    if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                        break DesktopResponse::Error {
                            code: "timeout".into(),
                            message: "wait_for timed out".into(),
                        };
                    }
                    let (reply, result) = mpsc::sync_channel(1);
                    let poll_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
                    if tx
                        .try_send(Pending {
                            request: DesktopRequest {
                                v: DESKTOP_CONTROL_VERSION,
                                action: DesktopAction::State {
                                    target: target.clone(),
                                },
                            },
                            reply,
                            deadline: poll_deadline,
                            cancelled: Arc::default(),
                        })
                        .is_err()
                    {
                        break DesktopResponse::Error {
                            code: "busy".into(),
                            message: "desktop request queue is full or closed".into(),
                        };
                    }
                    let response = loop {
                        if stop.load(Ordering::Acquire) || Instant::now() >= poll_deadline {
                            break None;
                        }
                        match result.recv_timeout(Duration::from_millis(10)) {
                            Ok(response) => break Some(response),
                            Err(mpsc::RecvTimeoutError::Timeout) => (),
                            Err(_) => break None,
                        }
                    };
                    match response {
                        Some(DesktopResponse::Ok { result })
                            if jcode_sdk::desktop_control::condition_met(&condition, &result) =>
                        {
                            break DesktopResponse::Ok { result };
                        }
                        Some(DesktopResponse::Ok { .. }) => (),
                        Some(error) => break error,
                        None => {
                            break DesktopResponse::Error {
                                code: "timeout".into(),
                                message: "desktop state request timed out".into(),
                            };
                        }
                    }
                    thread::sleep(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(100)),
                    );
                };
                let _ = writeln!(
                    socket,
                    "{}",
                    serde_json::to_string(&response).unwrap_or_default()
                );
                waiters.fetch_sub(1, Ordering::AcqRel);
            });
            return Ok(());
        }
        let (reply, result) = mpsc::sync_channel(1);
        tx.try_send(Pending {
            request,
            reply,
            deadline,
            cancelled: Arc::default(),
        })
        .map_err(io::Error::other)?;
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(io::Error::other("UI response timed out"));
            }
            match result.recv_timeout(Duration::from_millis(10)) {
                Ok(response) => {
                    return writeln!(
                        stream,
                        "{}",
                        serde_json::to_string(&response).map_err(io::Error::other)?
                    );
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(e) => return Err(io::Error::other(e)),
            }
        }
    }

    #[allow(deprecated, reason = "try_update is newer than the supported MSRV")]
    fn reserve(waiters: &AtomicUsize) -> bool {
        waiters
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 8).then_some(count + 1)
            })
            .is_ok()
    }

    fn computer(
        stream: &mut UnixStream,
        request: DesktopRequest,
        tx: &async_channel::Sender<Pending>,
        stop: &AtomicBool,
    ) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(COMPUTER_USE_TIMEOUT_SECS);
        let cancelled = Arc::new(AtomicBool::new(false));
        let (reply, responses) = mpsc::sync_channel(1);
        let queued = tx.try_send(Pending {
            request,
            reply,
            deadline,
            cancelled: cancelled.clone(),
        });
        let error = |code: &str, message: &str| DesktopResponse::Error {
            code: code.into(),
            message: message.into(),
        };
        let response = if queued.is_err() {
            error("busy", "Desktop request queue is full or closed")
        } else {
            loop {
                if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                    break error("desktop_timeout", "Desktop computer use timed out");
                }
                match responses.try_recv() {
                    Ok(response) => break response,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break error(
                            "desktop_unavailable",
                            "Desktop stopped handling the request",
                        );
                    }
                    Err(mpsc::TryRecvError::Empty) => (),
                }
                // This one-request protocol expects no more bytes. EOF means the
                // client gave up before consent or execution could begin.
                match stream.read(&mut [0]) {
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                    _ => {
                        cancelled.store(true, Ordering::Release);
                        return Ok(());
                    }
                }
            }
        };
        cancelled.store(true, Ordering::Release);
        writeln!(stream, "{}", serde_json::to_string(&response)?)
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _ = self.stop_accept.shutdown(Shutdown::Write);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            if std::fs::symlink_metadata(&self.path)
                .is_ok_and(|meta| (meta.dev(), meta.ino()) == self.identity)
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
#[cfg(target_os = "macos")]
pub(crate) use unix::Pending;
#[cfg(unix)]
pub(crate) use unix::Server;
