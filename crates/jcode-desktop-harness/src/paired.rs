//! Paired destinations contain no credentials. Tokens live only in the OS keychain.
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::Mutex};

const SERVICE: &str = "com.jcode.desktop.paired-harness";
/// A debug build started with this set to `1` may pair with a loopback `http`
/// gateway and keeps the token in a file under `$JCODE_HOME` instead of the
/// Keychain, so a private daemon on this machine can stand in for a paired
/// one. Release builds ignore it: pairing is HTTPS only.
pub const DEBUG_LOOPBACK_ENV: &str = "JCODE_DESKTOP_DEBUG_LOOPBACK_PAIRING";
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Destination {
    pub id: String,
    pub endpoint: String,
    pub label: String,
    pub working_dir: String,
}

// Labels of paired machines, so every surface names one by what the owner
// called it and never by its internal `paired:<id>`. Per thread: the UI thread
// publishes and reads them all, and a process-wide list let parallel tests
// overwrite each other's machines.
thread_local! {
    static LABELS: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub fn publish_labels(paired: &[Destination]) {
    LABELS.with_borrow_mut(|labels| {
        *labels = paired.iter().map(|d| (d.host(), d.label.clone())).collect();
    });
}

/// The owner's name for a paired destination; `None` for any other host. A
/// machine unpaired since a session was saved still reads as a machine, never as its id.
pub fn label(host: &str) -> Option<String> {
    crate::remote_targets::paired_id(host)?;
    Some(LABELS.with_borrow(|labels| {
        labels
            .iter()
            .find(|(known, _)| known == host)
            .map_or_else(|| "Paired machine".to_owned(), |(_, label)| label.clone())
    }))
}

#[cfg(debug_assertions)]
fn debug_loopback_pairing() -> bool {
    std::env::var(DEBUG_LOOPBACK_ENV).as_deref() == Ok("1")
}

/// Compiled out of release builds: nothing there can switch it on.
#[cfg(not(debug_assertions))]
fn debug_loopback_pairing() -> bool {
    false
}

/// HTTPS always; plain HTTP only to a loopback address, and only when `loopback_http`.
fn scheme_allowed(url: &reqwest::Url, loopback_http: bool) -> bool {
    url.scheme() == "https"
        || (loopback_http
            && url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
            }))
}

impl Destination {
    pub fn new(endpoint: &str, label: &str, working_dir: &str) -> Result<Self, String> {
        Self::with_id(
            uuid::Uuid::new_v4().simple().to_string(),
            endpoint,
            label,
            working_dir,
        )
    }

    /// Validate the same fields for a machine that keeps its id, such as one
    /// that is paired again after the gateway revoked its token.
    pub fn with_id(
        id: String,
        endpoint: &str,
        label: &str,
        working_dir: &str,
    ) -> Result<Self, String> {
        let url =
            reqwest::Url::parse(endpoint.trim()).map_err(|_| "Enter a valid HTTPS endpoint")?;
        if !scheme_allowed(&url, debug_loopback_pairing())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("Endpoint must be HTTPS without credentials, query, or fragment".into());
        }
        let label = label.trim();
        let working_dir = working_dir.trim();
        if label.is_empty() || label.chars().any(char::is_control) {
            return Err("Enter a device label".into());
        }
        if !working_dir.starts_with('/') || working_dir.chars().any(char::is_control) {
            return Err("Enter an absolute remote project path".into());
        }
        Ok(Self {
            id,
            endpoint: url.to_string().trim_end_matches('/').to_owned(),
            label: label.into(),
            working_dir: working_dir.into(),
        })
    }
    pub fn host(&self) -> String {
        format!("paired:{}", self.id)
    }
}

fn path() -> PathBuf {
    let home = std::env::var_os("JCODE_HOME")
        .map(PathBuf::from)
        .or_else(jcode_desktop_model::platform_dirs::jcode_home)
        .unwrap_or_else(|| PathBuf::from(".jcode"));
    home.join("desktop-paired-machines.json")
}

pub fn list() -> Result<Vec<Destination>, String> {
    match fs::read(path()) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid paired machines file".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("Could not read paired machines".into()),
    }
}

pub fn find(host: &str) -> Result<Destination, String> {
    let id = crate::remote_targets::paired_id(host).ok_or("Invalid paired machine ID")?;
    let destination = list()?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| "Paired machine not found".to_owned())?;
    Destination::new(
        &destination.endpoint,
        &destination.label,
        &destination.working_dir,
    )?;
    Ok(destination)
}

pub(crate) fn write_all(destinations: &[Destination]) -> Result<(), String> {
    let path = path();
    let dir = path.parent().ok_or("Invalid paired machines path")?;
    fs::create_dir_all(dir).map_err(|_| "Could not create paired machines directory")?;
    let mut file =
        tempfile::NamedTempFile::new_in(dir).map_err(|_| "Could not save paired machine")?;
    serde_json::to_writer(&mut file, destinations)
        .map_err(|_| "Could not encode paired machine")?;
    file.flush().map_err(|_| "Could not save paired machine")?;
    file.persist(path)
        .map_err(|_| "Could not save paired machine")?;
    Ok(())
}

pub fn save(destination: &Destination, token: &str) -> Result<(), String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| "Paired machine storage unavailable")?;
    let mut destinations = list()?;
    if destinations.iter().any(|d| d.id == destination.id) {
        return Err("Duplicate paired machine ID".into());
    }
    set_token(&destination.id, token)?;
    destinations.push(destination.clone());
    let result = write_all(&destinations);
    if result.is_err() {
        let _ = delete_token(&destination.id);
    }
    result
}

/// Replace the token and details of an already paired machine, keeping its id
/// so sessions and the default keep pointing at it.
pub fn save_repaired(destination: &Destination, token: &str) -> Result<(), String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| "Paired machine storage unavailable")?;
    let mut destinations = list()?;
    let slot = destinations
        .iter_mut()
        .find(|d| d.id == destination.id)
        .ok_or("Paired machine not found")?;
    *slot = destination.clone();
    set_token(&destination.id, token)?;
    write_all(&destinations)
}

/// Change the project path new sessions on this machine start in.
pub fn set_working_dir(id: &str, working_dir: &str) -> Result<Destination, String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| "Paired machine storage unavailable")?;
    let mut destinations = list()?;
    let slot = destinations
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or("Paired machine not found")?;
    let updated = Destination::with_id(slot.id.clone(), &slot.endpoint, &slot.label, working_dir)?;
    *slot = updated.clone();
    write_all(&destinations)?;
    Ok(updated)
}

/// Tests keep tokens in memory and never touch the real Keychain.
#[cfg(test)]
static TEST_TOKENS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Run `f` with `JCODE_HOME` pointing at a fresh directory. Serialized, because
/// the environment is process-wide.
#[cfg(test)]
pub(crate) fn with_home<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    static HOME_LOCK: Mutex<()> = Mutex::new(());
    let _guard = HOME_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = tempfile::tempdir().unwrap();
    // SAFETY: serialized by HOME_LOCK against every other user of this helper.
    unsafe { std::env::set_var("JCODE_HOME", home.path()) };
    let result = f(home.path());
    unsafe { std::env::remove_var("JCODE_HOME") };
    unsafe { std::env::remove_var(DEBUG_LOOPBACK_ENV) };
    result
}

/// Allow loopback HTTP pairing for the rest of the enclosing `with_home`,
/// the way a debug build started with the variable would.
#[cfg(test)]
pub(crate) fn enable_debug_loopback() {
    // SAFETY: only called inside `with_home`, which serializes environment use.
    unsafe { std::env::set_var(DEBUG_LOOPBACK_ENV, "1") };
}

#[cfg(test)]
pub(crate) fn set_test_token(id: &str, token: &str) {
    let mut tokens = TEST_TOKENS.lock().unwrap();
    tokens.retain(|(known, _)| known != id);
    tokens.push((id.to_owned(), token.to_owned()));
}

fn set_token(id: &str, token: &str) -> Result<(), String> {
    #[cfg(all(debug_assertions, not(test)))]
    if debug_loopback_pairing() {
        return debug_tokens::set(id, token);
    }
    os_set_token(id, token)
}

pub fn token(id: &str) -> Result<String, String> {
    #[cfg(all(debug_assertions, not(test)))]
    if debug_loopback_pairing() {
        return debug_tokens::get(id);
    }
    os_token(id)
}

fn delete_token(id: &str) -> Result<(), String> {
    #[cfg(all(debug_assertions, not(test)))]
    if debug_loopback_pairing() {
        return debug_tokens::delete(id);
    }
    os_delete_token(id)
}

/// Tokens of private test machines, never the Keychain: an unsigned debug
/// build prompts for it, and a throwaway token should not outlive its daemon.
/// Tests keep tokens in memory like every other paired test, so only the
/// store itself is exercised there.
#[cfg(debug_assertions)]
mod debug_tokens {
    use std::collections::BTreeMap;

    fn file() -> std::path::PathBuf {
        super::path().with_file_name("desktop-paired-tokens-debug.json")
    }

    fn load() -> BTreeMap<String, String> {
        std::fs::read(file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn store(tokens: &BTreeMap<String, String>) -> Result<(), String> {
        use std::io::Write;
        let path = file();
        let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid token path")?)
            .map_err(|_| "Could not save token")?;
        serde_json::to_writer(&mut file, tokens).map_err(|_| "Could not encode token")?;
        file.flush().map_err(|_| "Could not save token")?;
        file.persist(path).map_err(|_| "Could not save token")?;
        Ok(())
    }

    pub(super) fn set(id: &str, token: &str) -> Result<(), String> {
        let mut tokens = load();
        tokens.insert(id.to_owned(), token.to_owned());
        store(&tokens)
    }

    pub(super) fn get(id: &str) -> Result<String, String> {
        load()
            .remove(id)
            .ok_or_else(|| "Paired token unavailable".into())
    }

    pub(super) fn delete(id: &str) -> Result<(), String> {
        let mut tokens = load();
        tokens.remove(id);
        store(&tokens)
    }
}

#[cfg(target_os = "macos")]
fn os_set_token(id: &str, token: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        set_test_token(id, token);
        return Ok(());
    }
    #[cfg(not(test))]
    security_framework::passwords::set_generic_password(SERVICE, id, token.as_bytes())
        .map_err(|_| "Could not save token in Keychain".into())
}
#[cfg(target_os = "macos")]
fn os_token(id: &str) -> Result<String, String> {
    #[cfg(test)]
    {
        TEST_TOKENS
            .lock()
            .unwrap()
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, token)| token.clone())
            .ok_or_else(|| "Paired token unavailable in Keychain".into())
    }
    #[cfg(not(test))]
    {
        let bytes = security_framework::passwords::get_generic_password(SERVICE, id)
            .map_err(|_| "Paired token unavailable in Keychain")?;
        String::from_utf8(bytes).map_err(|_| "Invalid paired token".into())
    }
}
#[cfg(target_os = "macos")]
fn os_delete_token(id: &str) -> Result<(), String> {
    security_framework::passwords::delete_generic_password(SERVICE, id)
        .map_err(|_| "Could not remove token from Keychain".into())
}
#[cfg(not(target_os = "macos"))]
fn os_set_token(_: &str, _: &str) -> Result<(), String> {
    Err("Paired machines require macOS Keychain".into())
}
#[cfg(not(target_os = "macos"))]
fn os_token(_: &str) -> Result<String, String> {
    Err("Paired machines require macOS Keychain".into())
}
#[cfg(not(target_os = "macos"))]
fn os_delete_token(_: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destinations_validate_and_never_serialize_credentials() {
        for endpoint in [
            "http://host",
            "https://user:secret@host",
            "https://host/?token=secret",
            "https://host/#secret",
        ] {
            assert!(Destination::new(endpoint, "Work", "/work").is_err());
        }
        assert!(Destination::new("https://host", "Work", "relative").is_err());
        let d = Destination::new("https://host", "Work", "/work").unwrap();
        assert!(crate::remote_targets::paired_id(&d.host()).is_some());
        assert!(!serde_json::to_string(&d).unwrap().contains("token"));
    }

    fn url(text: &str) -> reqwest::Url {
        reqwest::Url::parse(text).unwrap()
    }

    #[test]
    fn plain_http_is_refused_unless_loopback_http_is_explicitly_allowed() {
        // The release rule: nothing allows it, so only HTTPS is accepted.
        for http in ["http://127.0.0.1:7643", "http://[::1]:7643", "http://work.example"] {
            assert!(!scheme_allowed(&url(http), false), "{http}");
        }
        assert!(scheme_allowed(&url("https://work.example"), false));
        // The debug allowance reaches loopback only, never another host.
        assert!(scheme_allowed(&url("http://127.0.0.1:7643"), true));
        assert!(scheme_allowed(&url("http://[::1]:7643"), true));
        assert!(!scheme_allowed(&url("http://work.example"), true));
        assert!(!scheme_allowed(&url("http://192.168.1.5"), true));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_pairs_over_loopback_http_only_with_the_variable() {
        with_home(|_| {
            let loopback = "http://127.0.0.1:7643";
            assert!(Destination::new(loopback, "Work", "/work").is_err());
            enable_debug_loopback();
            assert!(Destination::new(loopback, "Work", "/work").is_ok());
            // Even then a remote host must be HTTPS.
            assert!(Destination::new("http://work.example", "Work", "/work").is_err());
        });
        // The allowance ends with the scope that enabled it.
        with_home(|_| assert!(Destination::new("http://127.0.0.1:7643", "Work", "/work").is_err()));
    }

    /// The release rule: no environment can switch loopback HTTP on.
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_build_ignores_the_loopback_variable_and_pairs_over_https_only() {
        with_home(|_| {
            enable_debug_loopback();
            assert!(Destination::new("http://127.0.0.1:7643", "Work", "/work").is_err());
            assert!(Destination::new("https://work.example", "Work", "/work").is_ok());
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_tokens_stay_in_a_private_file_under_the_home_not_the_keychain() {
        with_home(|home| {
            debug_tokens::set("m1", "token-one").unwrap();
            debug_tokens::set("m2", "token-two").unwrap();
            assert_eq!(debug_tokens::get("m1").unwrap(), "token-one");
            debug_tokens::delete("m1").unwrap();
            assert!(debug_tokens::get("m1").is_err());
            assert_eq!(debug_tokens::get("m2").unwrap(), "token-two");
            let file = home.join("desktop-paired-tokens-debug.json");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o777, 0o600);
            }
        });
    }

    #[test]
    fn working_dir_is_editable_per_machine_and_validated() {
        with_home(|_| {
            let a = Destination::new("https://a.example", "A", "/a").unwrap();
            let b = Destination::new("https://b.example", "B", "/b").unwrap();
            write_all(&[a.clone(), b.clone()]).unwrap();
            let updated = set_working_dir(&a.id, " /a/new ").unwrap();
            assert_eq!(updated.working_dir, "/a/new");
            assert_eq!(updated.label, "A");
            assert!(set_working_dir(&a.id, "relative").is_err());
            let listed = list().unwrap();
            assert_eq!(listed[0].working_dir, "/a/new");
            assert_eq!(listed[1].working_dir, "/b");
            assert!(set_working_dir("missing", "/x").is_err());
        });
    }
}
