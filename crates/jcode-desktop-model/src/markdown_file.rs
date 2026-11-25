use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    External(String),
    Local {
        path: PathBuf,
        fragment: Option<String>,
    },
    Anchor(String),
}

/// Resolve a Markdown destination without accessing the filesystem.
pub fn resolve(base_dir: &Path, destination: &str) -> Option<LinkTarget> {
    if destination.is_empty() || destination.chars().any(char::is_control) {
        return None;
    }

    if let (false, Some(colon)) = (is_drive_path(destination), destination.find(':')) {
        let prefix = &destination[..colon];
        if !prefix.contains(['/', '#', '?']) {
            if prefix.eq_ignore_ascii_case("http")
                || prefix.eq_ignore_ascii_case("https")
                || prefix.eq_ignore_ascii_case("mailto")
            {
                return Some(LinkTarget::External(destination.to_owned()));
            }
            if prefix.eq_ignore_ascii_case("file") {
                return resolve_file_url(base_dir, &destination[colon + 1..]);
            }
            return None;
        }
    }

    resolve_local(base_dir, destination, is_drive_path(destination))
}

#[derive(Default)]
pub struct FileReferenceCache {
    files: HashMap<PathBuf, (Instant, bool)>,
    last_prune: Option<Instant>,
}

impl FileReferenceCache {
    /// Transcript paths are literal filenames, not URL-encoded destinations.
    pub fn resolve(&mut self, base_dir: &Path, mention: &str) -> Option<PathBuf> {
        let path = reference_path(base_dir, mention, crate::platform_dirs::home_dir)?;
        let now = crate::fs_probe::now();
        let freshness = Duration::from_secs(5);
        if let Some(&(checked, exists)) = self.files.get(&path)
            && now.duration_since(checked) < freshness
        {
            return exists.then_some(path);
        }
        if self.files.len() >= 2048
            && self
                .last_prune
                .is_none_or(|time| now.duration_since(time) >= freshness)
        {
            self.files
                .retain(|_, (checked, _)| now.duration_since(*checked) < freshness);
            self.last_prune = Some(now);
        }
        let exists = crate::fs_probe::is_file(&path);
        self.files.insert(path.clone(), (now, exists));
        exists.then_some(path)
    }
}

fn reference_path(
    base_dir: &Path,
    mention: &str,
    home_dir: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    let path = mention
        .rsplit_once(':')
        .filter(|(_, line)| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
        .map_or(mention, |(path, _)| path);
    let file = Path::new(path);
    let has_separator = path.contains('/') || (cfg!(windows) && path.contains('\\'));
    if file.file_name().is_none()
        || path.contains("://")
        || path.chars().any(char::is_control)
        || (!has_separator && file.extension().is_none_or(|ext| ext.is_empty()))
    {
        return None;
    }
    if let Some(path) = path.strip_prefix("~/") {
        Some(home_dir()?.join(path))
    } else {
        Some(base_dir.join(file))
    }
}

fn resolve_file_url(base_dir: &Path, rest: &str) -> Option<LinkTarget> {
    let rest = rest.strip_prefix("//")?;
    let slash = rest.find('/')?;
    let authority = &rest[..slash];
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return None;
    }
    let path = &rest[slash..];
    resolve_local(base_dir, path, false)
}

fn resolve_local(base_dir: &Path, destination: &str, drive_path: bool) -> Option<LinkTarget> {
    let (path, fragment) = match destination.split_once('#') {
        Some((path, fragment)) => (path, Some(percent_decode(fragment)?)),
        None => (destination, None),
    };
    if path.is_empty() {
        return fragment.map(LinkTarget::Anchor);
    }
    let path = PathBuf::from(percent_decode(path)?);
    let path = if path.is_absolute() || drive_path {
        path
    } else {
        base_dir.join(path)
    };
    Some(LinkTarget::Local { path, fragment })
}

fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
            let byte = (hex(*bytes.get(index + 1)?)? << 4) | hex(*bytes.get(index + 2)?)?;
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.chars().any(char::is_control)).then_some(decoded)
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(path: &str, fragment: Option<&str>) -> Option<LinkTarget> {
        Some(LinkTarget::Local {
            path: PathBuf::from(path),
            fragment: fragment.map(str::to_owned),
        })
    }

    #[test]
    fn resolves_paths_and_fragments() {
        let base = Path::new("/reports/daily");
        assert_eq!(
            resolve(base, "../hello world.md#A%20heading"),
            local("/reports/daily/../hello world.md", Some("A heading"))
        );
        assert_eq!(
            resolve(base, "image%23one%20%E2%98%83.png"),
            local("/reports/daily/image#one ☃.png", None)
        );
        assert_eq!(resolve(base, "/tmp/test.md"), local("/tmp/test.md", None));
        assert_eq!(
            resolve(base, "#some%20heading"),
            Some(LinkTarget::Anchor("some heading".into()))
        );
        assert_eq!(
            resolve(base, "C:\\docs\\file.md"),
            local("C:\\docs\\file.md", None)
        );
    }

    #[test]
    fn transcript_references_require_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let report = base.join("daily #1%.md");
        let directory = base.join("docs.md");
        let extensionless = directory.join("report");
        std::fs::write(&report, "# Report").unwrap();
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(&extensionless, "Report").unwrap();
        let mut cache = FileReferenceCache::default();

        assert_eq!(cache.resolve(base, "daily #1%.md:12"), Some(report.clone()));
        assert_eq!(
            cache.resolve(&directory, report.to_str().unwrap()),
            Some(report)
        );
        assert_eq!(cache.resolve(base, "docs.md/report"), Some(extensionless));
        assert_eq!(cache.resolve(&directory, "daily #1%.md"), None);
        assert_eq!(cache.resolve(base, "missing.md"), None);
        assert_eq!(cache.resolve(base, "docs.md"), None);
        assert_eq!(cache.resolve(&directory, "report"), None);
        assert_eq!(cache.resolve(base, "https://example.com/report.md"), None);
        assert_eq!(
            reference_path(&directory, "~/daily #1%.md:12", || Some(base.into())),
            Some(base.join("daily #1%.md")),
        );
    }

    #[test]
    fn permits_known_schemes_and_local_file_urls_only() {
        let base = Path::new("/reports");
        for url in [
            "https://example.com/a#b",
            "HTTP://example.com",
            "mailto:a@example.com",
        ] {
            assert_eq!(resolve(base, url), Some(LinkTarget::External(url.into())));
        }
        assert_eq!(
            resolve(base, "file:///tmp/a%20b.md#heading"),
            local("/tmp/a b.md", Some("heading"))
        );
        assert_eq!(
            resolve(base, "file://localhost/tmp/a.md"),
            local("/tmp/a.md", None)
        );
        for url in [
            "file://remote/tmp/a.md",
            "file:relative.md",
            "javascript:alert(1)",
            "data:text/plain,x",
            "unknown:thing",
        ] {
            assert_eq!(resolve(base, url), None, "{url}");
        }
    }

    #[test]
    fn rejects_bad_encodings_and_controls() {
        let base = Path::new("/reports");
        for destination in ["", "bad%", "bad%xy", "%FF", "%00", "line\nbreak", "#%FF"] {
            assert_eq!(resolve(base, destination), None, "{destination:?}");
        }
    }

    #[test]
    fn identifies_markdown_extensions() {
        for path in ["a.md", "a.MD", "a.markdown", "a.MarkDown"] {
            assert!(is_markdown(Path::new(path)));
        }
        for path in ["a", "a.md.png", "a.mdx"] {
            assert!(!is_markdown(Path::new(path)));
        }
    }
}
