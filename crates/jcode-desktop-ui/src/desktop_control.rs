//! Local, per-window Desktop control transport. Requests are executed on GPUI's
//! window thread, never on this socket thread.
#[cfg(unix)]
mod unix {
    use jcode_sdk::desktop_control::{DesktopRequest, DesktopResponse};
    use std::{
        io::{self, Read, Write},
        os::unix::{
            fs::{DirBuilderExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    pub struct Pending {
        pub request: DesktopRequest,
        pub reply: mpsc::SyncSender<DesktopResponse>,
        pub deadline: Instant,
    }

    pub struct Server {
        path: PathBuf,
        identity: (u64, u64),
        stop: Arc<AtomicBool>,
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
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let (tx, rx) = async_channel::bounded(8);
            let thread = thread::spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if let Err(e) = handle(&mut stream, &tx, &worker_stop) {
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
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10))
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
        stop: &AtomicBool,
    ) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(100)))?;
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
        let request = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let (reply, result) = mpsc::sync_channel(1);
        tx.try_send(Pending {
            request,
            reply,
            deadline,
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

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
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
#[cfg(unix)]
pub(crate) use unix::Server;
