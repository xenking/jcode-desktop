//! Paired destinations contain no credentials. Tokens live only in the OS keychain.
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::Mutex};

const SERVICE: &str = "com.jcode.desktop.paired-harness";
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Destination {
    pub id: String,
    pub endpoint: String,
    pub label: String,
    pub working_dir: String,
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
        if url.scheme() != "https"
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
    Destination::new(&destination.endpoint, &destination.label, &destination.working_dir)?;
    Ok(destination)
}

fn write_all(destinations: &[Destination]) -> Result<(), String> {
    let path = path();
    let dir = path.parent().ok_or("Invalid paired machines path")?;
    fs::create_dir_all(dir).map_err(|_| "Could not create paired machines directory")?;
    let mut file =
        tempfile::NamedTempFile::new_in(dir).map_err(|_| "Could not save paired machine")?;
    serde_json::to_writer(&mut file, destinations).map_err(|_| "Could not encode paired machine")?;
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
    let updated = Destination::with_id(
        slot.id.clone(),
        &slot.endpoint,
        &slot.label,
        working_dir,
    )?;
    *slot = updated.clone();
    write_all(&destinations)?;
    Ok(updated)
}

#[cfg(target_os = "macos")]
fn set_token(id: &str, token: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(SERVICE, id, token.as_bytes())
        .map_err(|_| "Could not save token in Keychain".into())
}
#[cfg(target_os = "macos")]
pub fn token(id: &str) -> Result<String, String> {
    let bytes = security_framework::passwords::get_generic_password(SERVICE, id)
        .map_err(|_| "Paired token unavailable in Keychain")?;
    String::from_utf8(bytes).map_err(|_| "Invalid paired token".into())
}
#[cfg(target_os = "macos")]
fn delete_token(id: &str) -> Result<(), String> {
    security_framework::passwords::delete_generic_password(SERVICE, id)
        .map_err(|_| "Could not remove token from Keychain".into())
}
#[cfg(not(target_os = "macos"))]
fn set_token(_: &str, _: &str) -> Result<(), String> {
    Err("Paired machines require macOS Keychain".into())
}
#[cfg(not(target_os = "macos"))]
pub fn token(_: &str) -> Result<String, String> {
    Err("Paired machines require macOS Keychain".into())
}
#[cfg(not(target_os = "macos"))]
fn delete_token(_: &str) -> Result<(), String> {
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

    #[test]
    fn working_dir_is_editable_per_machine_and_validated() {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: the only test in this crate touching JCODE_HOME for paired machines.
        unsafe { std::env::set_var("JCODE_HOME", home.path()) };
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
        unsafe { std::env::remove_var("JCODE_HOME") };
    }
}
