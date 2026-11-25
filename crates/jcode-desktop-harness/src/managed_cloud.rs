//! Managed Jcode Cloud transport.
//!
//! The Cloud destination is Jcode-operated infrastructure authorized by the
//! user's Jcode account. No AWS credential, SSH config, agent, or personal key
//! is involved. For each connection Desktop generates a fresh ed25519 key that
//! the control plane authorizes on the account's own host for 60 seconds, then
//! connects with that key only, pinned to host keys the control plane captured
//! at the host's first boot.

pub mod parity;

use jcode_sdk::{JcodeClient, SshConnectOptions};
use serde::Deserialize;
use std::path::Path;
use std::time::{Duration, Instant};

/// Reserved destination name. Never resolved through SSH config or DNS.
pub const HOST: &str = "jcode-cloud";
const NAMED_PREFIX: &str = "cloud:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Destination<'a>(Option<&'a str>);

impl<'a> Destination<'a> {
    pub fn parse(host: &'a str) -> Option<Self> {
        if host == HOST {
            return Some(Self(None));
        }
        Self::named(host.strip_prefix(NAMED_PREFIX)?)
    }

    pub fn named(name: &'a str) -> Option<Self> {
        let alpha_num = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        (name.len() <= 63
            && name.bytes().next().is_some_and(alpha_num)
            && name.bytes().all(|b| alpha_num(b) || b == b'-'))
        .then_some(Self(Some(name)))
    }

    pub fn name(self) -> Option<&'a str> {
        self.0
    }

    pub fn preference(self) -> String {
        self.0
            .map_or_else(|| HOST.into(), |name| format!("{NAMED_PREFIX}{name}"))
    }

    pub fn label(self) -> &'a str {
        self.0.unwrap_or("Jcode Cloud")
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegisteredMachine {
    pub name: String,
    #[serde(rename = "default")]
    pub is_default: bool,
    pub last_seen_at: Option<i64>,
}

#[derive(Deserialize)]
struct HostList {
    hosts: Vec<RegisteredMachine>,
}
/// Persistent project directory created by the managed host bootstrap.
pub const DEFAULT_WORKSPACE: &str = "/home/ec2-user/workspaces";
const DEFAULT_API_BASE: &str = "https://api.jcode.sh/v1";
/// First boot installs Jcode. Allow generous time before giving up.
const PROVISION_DEADLINE: Duration = Duration::from_secs(8 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Deserialize)]
pub struct ConnectReply {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub host_keys: Vec<String>,
    /// Set by a self-hosted control plane, whose machines the account
    /// registered itself: they keep their own logins and have no bootstrap
    /// workspace.
    #[serde(default)]
    pub self_hosted: bool,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorReply {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

/// Account credentials come from the same hosted-account file Jcode uses.
#[derive(Eq, PartialEq)]
pub struct Account {
    pub api_key: String,
    pub api_base: String,
}

pub fn account() -> Result<Account, String> {
    let api_key = jcode_base::subscription_catalog::configured_api_key()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            "Sign in to your Jcode account to use Jcode Cloud. No local session was opened."
                .to_string()
        })?;
    let api_base = jcode_base::subscription_catalog::configured_api_base()
        .filter(|base| base.starts_with("https://"))
        .unwrap_or_else(|| DEFAULT_API_BASE.into());
    Ok(Account {
        api_key: api_key.trim().to_owned(),
        api_base: api_base.trim_end_matches('/').to_owned(),
    })
}

/// A key pair valid for one connection. Files live in a private temporary
/// directory that is removed when this value drops.
pub struct EphemeralKey {
    dir: tempfile::TempDir,
    pub public_key: String,
}

impl EphemeralKey {
    pub fn generate() -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("jcode-cloud-")
            .tempdir()
            .map_err(|e| format!("Could not create a private key directory: {e}"))?;
        let key = dir.path().join("id");
        let output =
            crate::platform::no_console_window(&mut std::process::Command::new("ssh-keygen"))
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "jcode-desktop", "-f"])
                .arg(&key)
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("ssh-keygen is required for Jcode Cloud: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "Could not generate a connection key: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let public_key = std::fs::read_to_string(key.with_extension("pub"))
            .map_err(|e| format!("Could not read the connection key: {e}"))?
            .trim()
            .to_owned();
        Ok(Self { dir, public_key })
    }

    pub fn private_key(&self) -> std::path::PathBuf {
        self.dir.path().join("id")
    }

    fn known_hosts(&self) -> std::path::PathBuf {
        self.dir.path().join("known_hosts")
    }
}

/// Validate the reply before any value reaches an SSH command line.
pub fn validated_target(
    reply: &ConnectReply,
) -> Result<(String, u16, String, Vec<String>), String> {
    let address = reply
        .address
        .as_deref()
        .filter(|a| a.parse::<std::net::IpAddr>().is_ok())
        .ok_or("Jcode Cloud returned an invalid host address")?;
    let user = reply
        .user
        .as_deref()
        .filter(|u| {
            !u.is_empty()
                && u.len() <= 32
                && u.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or("Jcode Cloud returned an invalid user")?;
    let keys: Vec<String> = reply
        .host_keys
        .iter()
        .filter(|key| {
            let mut parts = key.split(' ');
            matches!(
                (parts.next(), parts.next(), parts.next()),
                (Some("ssh-ed25519" | "ecdsa-sha2-nistp256"), Some(blob), None)
                    if blob.len() <= 1024 && blob.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
            )
        })
        .cloned()
        .collect();
    if keys.is_empty() {
        return Err("Jcode Cloud did not provide verified host keys".into());
    }
    Ok((
        address.to_owned(),
        reply.port.unwrap_or(22),
        user.to_owned(),
        keys,
    ))
}

pub fn known_hosts_contents(address: &str, port: u16, keys: &[String]) -> String {
    let host = if port == 22 {
        address.to_owned()
    } else {
        format!("[{address}]:{port}")
    };
    keys.iter().map(|key| format!("{host} {key}\n")).collect()
}

/// One control-plane request. Idempotent: each call advances the host toward
/// readiness and, when ready, authorizes `public_key` for one login.
pub fn request_connect(
    account: &Account,
    destination: Destination<'_>,
    public_key: &str,
) -> Result<ConnectReply, String> {
    let mut body = serde_json::json!({ "public_key": public_key });
    if let Some(name) = destination.name() {
        body["host"] = name.into();
    }
    let response = control_plane_client()?
        .post(format!("{}/cloud/host/connect", account.api_base))
        .bearer_auth(&account.api_key)
        .json(&body)
        .send()
        .map_err(|e| format!("Could not reach Jcode Cloud: {e}"))?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if status.is_success() {
        let reply: ConnectReply = serde_json::from_str(&body)
            .map_err(|e| format!("Unexpected Jcode Cloud reply: {e}"))?;
        if let Some(name) = destination.name()
            && (!reply.self_hosted || reply.name.as_deref() != Some(name))
        {
            return Err(format!("Jcode Cloud did not confirm machine {name}"));
        }
        return Ok(reply);
    }
    Err(match serde_json::from_str::<ErrorReply>(&body) {
        Ok(reply) => describe_error(status.as_u16(), &reply.error.code, &reply.error.message),
        Err(_) => format!("Jcode Cloud request failed (HTTP {status})"),
    })
}

fn control_plane_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("jcode-desktop/{}", crate::client_version()))
        .build()
        .map_err(|e| e.to_string())
}

/// A missing registry is normal on the upstream hosted control plane.
pub fn list_machines(account: &Account) -> Result<Vec<RegisteredMachine>, String> {
    let response = control_plane_client()?
        .get(format!("{}/cloud/hosts", account.api_base))
        .bearer_auth(&account.api_key)
        .send()
        .map_err(|e| format!("Could not reach Jcode Cloud: {e}"))?;
    let status = response.status();
    match status.as_u16() {
        404 | 501 => Ok(Vec::new()),
        _ if status.is_success() => {
            let mut list: HostList = response
                .json()
                .map_err(|e| format!("Unexpected Cloud machine list: {e}"))?;
            let mut names = std::collections::HashSet::new();
            list.hosts.retain(|machine| {
                Destination::named(&machine.name).is_some() && names.insert(machine.name.clone())
            });
            Ok(list.hosts)
        }
        _ => Err(format!("Could not list Cloud machines (HTTP {status})")),
    }
}

fn describe_error(status: u16, code: &str, message: &str) -> String {
    match (status, code) {
        (401, _) => "Your Jcode sign-in has expired. Sign in again to use Jcode Cloud.".into(),
        (402, "cloud_not_entitled") => {
            "Jcode Cloud requires an active paid Jcode subscription.".into()
        }
        (402, "cloud_allowance_exhausted") => message.to_owned(),
        _ => message.to_owned(),
    }
}

/// A failed best-effort login sync must not be cached as a synchronized host.
pub(crate) fn connect_with_sync_status(
    progress: &mut dyn FnMut(&str),
    client_name: String,
    mut request: impl FnMut(&Account, &str) -> Result<ConnectReply, String>,
    open: impl FnOnce(SshConnectOptions, bool, bool) -> jcode_sdk::Result<JcodeClient>,
) -> Result<(JcodeClient, bool), String> {
    let account = account()?;
    let key = EphemeralKey::generate()?;
    let deadline = Instant::now() + PROVISION_DEADLINE;
    let mut last = String::new();
    let reply = loop {
        let reply = request(&account, &key.public_key)?;
        if reply.ready {
            break reply;
        }
        let message = reply
            .message
            .clone()
            .unwrap_or_else(|| format!("Jcode Cloud machine is {}…", reply.state));
        if message != last {
            progress(&message);
            last = message;
        }
        if Instant::now() >= deadline {
            return Err("Jcode Cloud did not become ready in time. Retry to keep waiting.".into());
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    let (address, port, user, host_keys) = validated_target(&reply)?;
    write_private(
        &key.known_hosts(),
        &known_hosts_contents(&address, port, &host_keys),
    )?;
    progress("Connecting to your Jcode Cloud machine…");
    let options = SshConnectOptions {
        // Temporarily disabled until inline tool responses have broader testing.
        supports_user_response: false,
        supports_applets: true,
        port: Some(port),
        user: Some(user),
        identity_file: Some(key.private_key()),
        known_hosts_file: Some(key.known_hosts()),
        isolated_config: true,
        client_name,
        connect_timeout: Duration::from_secs(45),
        request_timeout: Some(Duration::from_secs(30)),
        ..SshConnectOptions::new(address)
    };
    // Mirror this computer's default model and logins before the daemon
    // serves the new session. A failure is shown but never blocks the session.
    // A self-hosted machine is the user's own: its daemon also serves other
    // clients, so its provider defaults and logins are left alone.
    let snapshot = crate::managed_cloud::parity::Snapshot::collect();
    let mut synchronized = true;
    if !reply.self_hosted && !snapshot.is_empty() {
        progress("Syncing your models and logins…");
        if let Err(message) = sync(&options, &snapshot) {
            synchronized = false;
            progress(&message);
        }
    }
    open(options, synchronized, reply.self_hosted)
        .map(|client| (client, reply.self_hosted))
        .map_err(|e| e.to_string())
    // `key` drops here. The authorized key expires server-side within 60s and
    // the established connection does not need the file again.
}

/// Model/login sync hook. Replaced in tests, which must never reach a host.
fn sync(
    options: &SshConnectOptions,
    snapshot: &crate::managed_cloud::parity::Snapshot,
) -> Result<(), String> {
    if cfg!(any(test, feature = "test-support")) || crate::platform::screenshot_mode() {
        return Ok(());
    }
    crate::managed_cloud::parity::apply(options, snapshot)
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(contents.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    fn reply(json: serde_json::Value) -> ConnectReply {
        serde_json::from_value(json).unwrap()
    }

    fn control_plane(
        replies: Vec<(
            &'static str,
            &'static str,
            Option<serde_json::Value>,
            u16,
            &'static str,
        )>,
    ) -> (Account, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api_base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (method, path, expected_body, code, body) in replies {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = BufReader::new(socket);
                let mut line = String::new();
                request.read_line(&mut line).unwrap();
                assert_eq!(line.trim_end(), format!("{method} /v1{path} HTTP/1.1"));
                let mut len = 0;
                loop {
                    line.clear();
                    assert_ne!(request.read_line(&mut line).unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = value.trim().parse().unwrap();
                    }
                }
                let mut bytes = vec![0; len];
                request.read_exact(&mut bytes).unwrap();
                if let Some(expected_body) = expected_body {
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                        expected_body
                    );
                } else {
                    assert!(bytes.is_empty());
                }
                write!(request.get_mut(), "HTTP/1.1 {code} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (
            Account {
                api_base,
                api_key: "test-key".into(),
            },
            server,
        )
    }

    #[test]
    fn upstream_backends_without_a_registry_keep_the_legacy_connect_request() {
        for code in [404, 501] {
            let (account, server) = control_plane(vec![
                ("GET", "/cloud/hosts", None, code, "{}"),
                (
                    "POST",
                    "/cloud/host/connect",
                    Some(serde_json::json!({"public_key":"test"})),
                    200,
                    r#"{"ready":true,"address":"203.0.113.7","user":"ec2-user","host_keys":["ssh-ed25519 AAAA"]}"#,
                ),
            ]);
            assert!(list_machines(&account).unwrap().is_empty());
            let reply =
                request_connect(&account, Destination::parse(HOST).unwrap(), "test").unwrap();
            assert!(reply.ready);
            assert!(!reply.self_hosted);
            assert_eq!(validated_target(&reply).unwrap().0, "203.0.113.7");
            server.join().unwrap();
        }
    }

    #[test]
    fn registry_lists_valid_names_and_connect_requires_the_same_named_machine() {
        let named = Some(serde_json::json!({"public_key":"test", "host":"ai-core"}));
        let (account, server) = control_plane(vec![
            (
                "GET",
                "/cloud/hosts",
                None,
                200,
                r#"{"hosts":[{"name":"ai-core","default":true,"last_seen_at":123},{"name":"studio","default":false,"last_seen_at":null},{"name":"user@rogue","default":false,"last_seen_at":null}]}"#,
            ),
            (
                "POST",
                "/cloud/host/connect",
                named.clone(),
                200,
                r#"{"name":"studio","ready":true,"self_hosted":true}"#,
            ),
            (
                "POST",
                "/cloud/host/connect",
                named,
                200,
                r#"{"name":"ai-core","ready":true,"self_hosted":true}"#,
            ),
        ]);
        let listed = list_machines(&account).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "ai-core");
        assert!(listed[0].is_default);
        assert_eq!(listed[0].last_seen_at, Some(123));
        assert_eq!(listed[1].name, "studio");
        assert_eq!(listed[1].last_seen_at, None);
        let destination = Destination::named("ai-core").unwrap();
        assert!(request_connect(&account, destination, "test").is_err());
        assert!(
            request_connect(&account, destination, "test")
                .unwrap()
                .ready
        );
        server.join().unwrap();
    }

    #[test]
    fn target_validation_rejects_anything_that_could_reach_ssh_options() {
        let good = reply(serde_json::json!({
            "state": "running", "ready": true, "address": "203.0.113.7", "port": 22,
            "user": "ec2-user", "host_keys": ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHostKey"]
        }));
        let (address, port, user, keys) = validated_target(&good).unwrap();
        assert_eq!(
            (address.as_str(), port, user.as_str()),
            ("203.0.113.7", 22, "ec2-user")
        );
        assert_eq!(keys.len(), 1);
        for bad in [
            serde_json::json!({"address": "-oProxyCommand=x", "user": "ec2-user", "host_keys": ["ssh-ed25519 AAAA"]}),
            serde_json::json!({"address": "evil.example", "user": "ec2-user", "host_keys": ["ssh-ed25519 AAAA"]}),
            serde_json::json!({"address": "203.0.113.7", "user": "-l root", "host_keys": ["ssh-ed25519 AAAA"]}),
            serde_json::json!({"address": "203.0.113.7", "user": "ec2-user", "host_keys": []}),
            serde_json::json!({"address": "203.0.113.7", "user": "ec2-user", "host_keys": ["ssh-ed25519 AAAA\n* ssh-ed25519 BBBB"]}),
            serde_json::json!({"address": "203.0.113.7", "user": "ec2-user", "host_keys": ["ssh-rsa AAAA"]}),
        ] {
            assert!(
                validated_target(&reply(bad.clone())).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn known_hosts_pins_only_the_published_address() {
        let keys = vec![
            "ssh-ed25519 AAAAC3".to_string(),
            "ecdsa-sha2-nistp256 AAAAE2".to_string(),
        ];
        assert_eq!(
            known_hosts_contents("203.0.113.7", 22, &keys),
            "203.0.113.7 ssh-ed25519 AAAAC3\n203.0.113.7 ecdsa-sha2-nistp256 AAAAE2\n"
        );
        assert!(
            known_hosts_contents("203.0.113.7", 2222, &keys).starts_with("[203.0.113.7]:2222 ")
        );
    }

    #[test]
    fn ephemeral_keys_are_fresh_private_and_removed_on_drop() {
        let first = EphemeralKey::generate().unwrap();
        let second = EphemeralKey::generate().unwrap();
        assert!(first.public_key.starts_with("ssh-ed25519 "));
        assert_ne!(first.public_key, second.public_key);
        let path = first.private_key();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        drop(first);
        assert!(!path.exists());
    }

    #[test]
    fn errors_explain_sign_in_and_subscription() {
        assert!(describe_error(401, "unauthorized", "x").contains("Sign in"));
        assert!(describe_error(402, "cloud_not_entitled", "x").contains("subscription"));
        assert_eq!(describe_error(503, "cloud_unavailable", "Down"), "Down");
    }
}
