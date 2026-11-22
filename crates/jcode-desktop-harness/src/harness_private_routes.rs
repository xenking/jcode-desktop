//! Opt-in, owner-local routing for seats on an existing private Jcode daemon.
//! OpenRig's network read model supplies the session ID, never the socket or home.

use jcode_sdk::{ConnectOptions, Error, ErrorKind, JcodeClient};
use serde::Deserialize;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const ROUTE_DIR: &str = "openrig-desktop-routes";
const MAX_ROUTE_BYTES: u64 = 4096;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Route {
    home: PathBuf,
    socket: PathBuf,
    socket_device: u64,
    socket_inode: u64,
}

fn local_session(id: &str) -> bool {
    id.starts_with("session_")
        && id.len() <= 200
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn owner_private(metadata: &Metadata) -> bool {
    // SAFETY: geteuid takes no pointers and has no preconditions.
    metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || !owner_private(&metadata) {
        return Err(invalid("expected an owner-only directory, not a symlink"));
    }
    Ok(())
}

fn trusted_ancestors(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| invalid("missing parent"))?;
    // SAFETY: geteuid takes no pointers and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(ancestor)?;
        let owner = metadata.uid();
        let mode = metadata.mode();
        let trusted_sticky = owner == 0 && mode & 0o1000 != 0;
        if !metadata.is_dir()
            || (owner != 0 && owner != uid)
            || (mode & 0o022 != 0 && !trusted_sticky)
        {
            return Err(invalid("route has an untrusted ancestor directory"));
        }
    }
    Ok(())
}

fn private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || !owner_private(&metadata) {
        return Err(invalid("expected an owner-only regular file"));
    }
    Ok(file)
}

fn load_route(desktop_home: &Path, session_id: &str) -> io::Result<Option<Route>> {
    if !local_session(session_id) {
        return Ok(None);
    }
    let directory = desktop_home.join(ROUTE_DIR);
    let route_path = directory.join(format!("{session_id}.json"));
    match fs::symlink_metadata(&route_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        result => {
            result?;
        }
    };
    trusted_ancestors(&route_path)?;
    private_directory(&directory)?;
    private_directory(desktop_home)?;
    let file = private_file(&route_path)?;
    let mut content = Vec::new();
    file.take(MAX_ROUTE_BYTES + 1).read_to_end(&mut content)?;
    if content.len() as u64 > MAX_ROUTE_BYTES {
        return Err(invalid("route file is too large"));
    }
    let route: Route = serde_json::from_slice(&content)?;
    if !route.home.is_absolute() || !route.socket.is_absolute() {
        return Err(invalid("route paths must be absolute"));
    }
    trusted_ancestors(&route.home)?;
    trusted_ancestors(&route.socket)?;
    private_directory(&route.home)?;
    if fs::canonicalize(&route.home)? == fs::canonicalize(desktop_home)? {
        return Err(invalid(
            "private route must not use the shared Desktop home",
        ));
    }
    private_directory(&route.home.join("sessions"))?;
    private_file(
        &route
            .home
            .join("sessions")
            .join(format!("{session_id}.json")),
    )?;
    private_directory(
        route
            .socket
            .parent()
            .ok_or_else(|| invalid("missing socket parent"))?,
    )?;
    let socket = fs::symlink_metadata(&route.socket)?;
    if !socket.file_type().is_socket() || !owner_private(&socket) {
        return Err(invalid("seat socket is not an owner-only Unix socket"));
    }
    if (socket.dev(), socket.ino()) != (route.socket_device, route.socket_inode) {
        return Err(invalid(
            "seat socket identity changed; register the live socket again",
        ));
    }
    Ok(Some(route))
}

struct PrivateBridge {
    route: Route,
    directory: tempfile::TempDir,
    child: Child,
}

impl PrivateBridge {
    fn start(route: Route) -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("jc-seat-").tempdir()?;
        let socket = directory.path().join("api.sock");
        let child = Command::new(crate::platform::companion_executable(
            "jcode-harness-api-bridge",
        ))
        .arg(&socket)
        .arg(&route.socket)
        .env("JCODE_HOME", &route.home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
        Ok(Self {
            route,
            directory,
            child,
        })
    }

    fn client(&self) -> jcode_sdk::Result<JcodeClient> {
        let socket = self.directory.path().join("api.sock");
        jcode_sdk::wait_for_socket(
            &socket,
            "Desktop private seat bridge",
            Duration::from_secs(2),
        )?;
        let client = JcodeClient::connect(ConnectOptions {
            socket_path: Some(socket),
            client_name: format!("jcode-desktop-private-seat/{}", crate::client_version()),
            request_timeout: Some(Duration::from_secs(10)),
            ensure_runtime: false,
            supports_user_response: false,
            supports_applets: true,
        })?;
        crate::runtime_generation::verify_capabilities(&client)?;
        Ok(client)
    }
}

impl Drop for PrivateBridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn route_error(error: io::Error) -> Error {
    Error::new(
        ErrorKind::ConnectFailed,
        format!("Private OpenRig seat route: {error}"),
    )
}

#[derive(Default)]
pub(super) struct PrivateRoute {
    bridge: Option<PrivateBridge>,
    was_configured: bool,
}

impl PrivateRoute {
    pub(super) fn connect(&mut self, session_id: &str) -> jcode_sdk::Result<Option<JcodeClient>> {
        let route = super::jcode_home()
            .map(|home| load_route(&home, session_id))
            .transpose()
            .map_err(route_error)?
            .flatten();
        let Some(route) = route else {
            self.bridge = None;
            return if self.was_configured {
                Err(route_error(invalid("route was withdrawn")))
            } else {
                Ok(None)
            };
        };
        self.was_configured = true;
        if let Some(existing) = &mut self.bridge
            && existing.route == route
            && existing.child.try_wait().map_err(route_error)?.is_none()
        {
            return existing.client().map(Some);
        }
        let bridge = PrivateBridge::start(route).map_err(route_error)?;
        let client = bridge.client()?;
        self.bridge = Some(bridge);
        Ok(Some(client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Update, spawn_session_worker, transport, update_channel};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    const ID: &str = "session_test_123";

    fn mode(path: &Path, bits: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
    }

    fn dir(path: PathBuf) -> PathBuf {
        fs::create_dir(&path).unwrap();
        mode(&path, 0o700);
        path
    }

    #[test]
    fn only_an_owned_exact_session_route_selects_a_private_socket() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let home = dir(root_path.join("desktop"));
        assert!(load_route(&home, ID).unwrap().is_none());
        let routes = dir(home.join(ROUTE_DIR));
        let private = dir(root_path.join("seat"));
        let sessions = dir(private.join("sessions"));
        let socket_parent = dir(root_path.join("endpoint"));
        let socket = socket_parent.join("daemon.sock");
        let _daemon = UnixListener::bind(&socket).unwrap();
        mode(&socket, 0o600);
        let socket_metadata = fs::symlink_metadata(&socket).unwrap();
        let registration = routes.join(format!("{ID}.json"));
        fs::write(
            &registration,
            serde_json::json!({
                "home":private,"socket":socket,
                "socket_device":socket_metadata.dev(),"socket_inode":socket_metadata.ino(),
            })
            .to_string(),
        )
        .unwrap();
        mode(&registration, 0o600);
        assert!(load_route(&home, ID).is_err()); // no saved native conversation
        let native = sessions.join(format!("{ID}.json"));
        fs::write(&native, "{}").unwrap();
        mode(&native, 0o600);
        assert_eq!(
            load_route(&home, ID).unwrap(),
            Some(Route {
                home: private,
                socket: socket.clone(),
                socket_device: socket_metadata.dev(),
                socket_inode: socket_metadata.ino(),
            })
        );
        for non_matching in [
            "ssh://other/session_test_123",
            "../session_test_123",
            "session_other_456",
        ] {
            assert!(load_route(&home, non_matching).unwrap().is_none());
        }
        mode(&socket, 0o666);
        assert!(load_route(&home, ID).is_err());
        mode(&socket, 0o600);
        let old_socket = socket_parent.join("old.sock");
        fs::rename(&socket, &old_socket).unwrap();
        let _substitute = UnixListener::bind(&socket).unwrap();
        mode(&socket, 0o600);
        let error = load_route(&home, ID).unwrap_err();
        assert!(error.to_string().contains("socket identity changed"));
        fs::rename(&socket, socket_parent.join("substitute.sock")).unwrap();
        symlink(&old_socket, &socket).unwrap();
        assert!(load_route(&home, ID).is_err());
        fs::remove_file(&socket).unwrap();
        fs::rename(&old_socket, &socket).unwrap();
        assert!(load_route(&home, ID).unwrap().is_some());
        mode(&root_path, 0o777);
        assert!(load_route(&home, ID).is_err());
        mode(&root_path, 0o700);
        let moved_parent = root_path.join("other-endpoint");
        fs::rename(&socket_parent, &moved_parent).unwrap();
        symlink(&moved_parent, &socket_parent).unwrap();
        let error = load_route(&home, ID).unwrap_err();
        assert!(error.to_string().contains("untrusted ancestor"));
        fs::remove_file(&socket_parent).unwrap();
        fs::rename(&moved_parent, &socket_parent).unwrap();
        mode(&registration, 0o644);
        assert!(load_route(&home, ID).is_err());
        let target = routes.join("registration");
        fs::rename(&registration, &target).unwrap();
        mode(&target, 0o600);
        symlink(&target, &registration).unwrap();
        assert!(load_route(&home, ID).is_err());
    }

    #[test]
    #[ignore = "requires an owner-registered live private seat and the production bridge companion"]
    fn desktop_worker_reads_registered_live_seat_history() {
        let session_id = std::env::var("JCODE_DESKTOP_LIVE_PRIVATE_SEAT").unwrap();
        let marker = std::env::var("JCODE_DESKTOP_LIVE_PRIVATE_MARKER").unwrap();
        let (updates, receiver) = update_channel();
        let worker = spawn_session_worker(
            session_id.clone(),
            &updates,
            &transport::RemoteTransports::default(),
        );
        let until = Instant::now() + Duration::from_secs(15);
        let observed = loop {
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break false;
            }
            match receiver.try_recv() {
                Ok(Update::BoundedHistory {
                    session_id: id,
                    page,
                }) if id == session_id => {
                    break serde_json::to_string(&page.messages)
                        .unwrap()
                        .contains(&marker);
                }
                Ok(Update::History {
                    session_id: id,
                    messages,
                    ..
                }) if id == session_id => {
                    break serde_json::to_string(&messages).unwrap().contains(&marker);
                }
                Ok(Update::SessionLost {
                    session_id: id,
                    reason,
                }) if id == session_id => {
                    panic!("the routed session lost its connection: {reason}");
                }
                Ok(_) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(25).min(remaining)),
            }
        };
        drop(worker);
        assert!(
            observed,
            "Desktop history did not include the expected marker"
        );
    }
}
