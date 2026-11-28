use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostTarget {
    alias: String,
    url: Option<Url>,
    credentials: Credentials,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Credentials {
    Primary,
    Anonymous,
    File(PathBuf),
    Environment(String),
}

#[derive(Deserialize)]
pub(crate) struct HostRegistry {
    #[serde(rename = "ownName")]
    pub(crate) own_name: Option<String>,
    hosts: Vec<HostEntry>,
}

#[derive(Deserialize)]
struct HostEntry {
    id: String,
    status: Option<String>,
    url: Option<String>,
    bearer_file: Option<PathBuf>,
    bearer_env: Option<String>,
}

impl HostRegistry {
    pub(crate) fn into_targets(self) -> impl Iterator<Item = (HostTarget, Option<String>)> {
        self.hosts.into_iter().filter_map(|entry| {
            if entry.id.trim().is_empty() || entry.id == "local" {
                return None;
            }
            let credentials = if let Some(env) = entry.bearer_env {
                Credentials::Environment(env)
            } else if let Some(path) = entry.bearer_file {
                Credentials::File(path)
            } else {
                Credentials::Anonymous
            };
            Some((
                HostTarget {
                    alias: entry.id,
                    url: entry.url.as_deref().and_then(parse_origin),
                    credentials,
                },
                entry.status,
            ))
        })
    }
}

fn parse_origin(base: &str) -> Option<Url> {
    let url = Url::parse(base).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

impl HostTarget {
    pub fn primary(base: &str, alias: &str) -> Self {
        Self {
            alias: alias.into(),
            url: parse_origin(base),
            credentials: Credentials::Primary,
        }
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn endpoint(&self) -> Option<&str> {
        self.url.as_ref().and_then(Url::host_str)
    }

    pub fn is_local_machine(&self) -> bool {
        self.is_primary() && matches!(self.endpoint(), Some("localhost" | "127.0.0.1" | "[::1]"))
    }

    pub(crate) fn is_primary(&self) -> bool {
        self.credentials == Credentials::Primary
    }

    pub(crate) fn bearer(&self, operator: bool) -> Result<Option<String>, String> {
        let value = match &self.credentials {
            Credentials::Primary => return Ok(crate::token(operator)),
            Credentials::Anonymous => return Ok(None),
            Credentials::File(path) => {
                std::fs::read_to_string(path).map_err(|error| match error.kind() {
                    std::io::ErrorKind::NotFound => "Bearer file is missing",
                    _ => "Bearer file is unreadable",
                })?
            }
            Credentials::Environment(name) => {
                std::env::var(name).map_err(|_| "Bearer environment variable is unavailable")?
            }
        };
        let value = value.trim();
        if value.is_empty() {
            return Err("Bearer credential is empty".into());
        }
        if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err("Bearer credential has invalid characters".into());
        }
        Ok(Some(value.into()))
    }

    pub(crate) fn request(
        &self,
        method: &str,
        route: &str,
        timeout: u64,
        bearer: Option<&str>,
    ) -> Result<ureq::Request, String> {
        let base = self.url.as_ref().ok_or("Missing or invalid daemon URL")?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout))
            .redirects(0)
            .build();
        let url = format!("{}{route}", base.as_str().trim_end_matches('/'));
        let mut request = agent.request(method, &url);
        if let Some(bearer) = bearer {
            request = request.set("Authorization", &format!("Bearer {bearer}"));
        }
        Ok(request)
    }
}

pub(crate) fn redact(value: &str, bearer: Option<&str>) -> String {
    match bearer {
        Some(token) if !token.is_empty() => value.replace(token, "[redacted]"),
        _ => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn registered_host_reads_rotated_bearer_at_request_time_and_redacts_errors() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("openrig-host-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        struct TempDir(PathBuf);
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = TempDir(dir);
        let path = dir.0.join("bearer");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let registry: HostRegistry = serde_json::from_value(json!({"hosts":[{
            "id":"remote", "url":url, "bearer_file":path
        }]}))
        .unwrap();
        let (target, _) = registry.into_targets().next().unwrap();
        assert!(!target.is_local_machine());
        assert_eq!(
            crate::get_target(&target, "/proof"),
            Err("Bearer file is missing".into())
        );

        for generation in 1..=2 {
            let token = format!("{nonce}-{generation}");
            let mut options = std::fs::OpenOptions::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            writeln!(file, "{token}").unwrap();
            std::thread::scope(|scope| {
                let server = scope.spawn(|| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut lines = BufReader::new(&stream).lines();
                    assert_eq!(lines.next().unwrap().unwrap(), "GET /proof HTTP/1.1");
                    let expected = format!("Bearer {token}");
                    let authenticated = lines.map_while(Result::ok).take_while(|line| !line.is_empty())
                        .any(|line| line.split_once(':').is_some_and(|(key, value)| {
                            key.eq_ignore_ascii_case("authorization") && value.trim() == expected
                        }));
                    let body = json!({"error": format!("refused {token}")}).to_string();
                    write!(stream, "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    assert!(authenticated, "remote did not get the current bearer");
                });
                assert_eq!(
                    crate::get_target(&target, "/proof"),
                    Err("/proof: HTTP 403 (refused [redacted])".into())
                );
                server.join().unwrap();
            });
        }
    }
}
