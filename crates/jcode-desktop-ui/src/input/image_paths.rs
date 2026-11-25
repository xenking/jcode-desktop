use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::file_drop::image_media_type;

fn image_path(text: &str) -> Option<PathBuf> {
    let path = Path::new(text);
    if !path.is_absolute() || image_media_type(path).is_none() || !path.is_file() {
        return None;
    }
    Some(path.to_path_buf())
}

pub(super) fn pasted_image_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let line = line.trim();
        // A single unquoted path copied from a file manager may contain spaces.
        if let Some(path) = image_path(line) {
            if seen.insert(path.clone()) {
                paths.push(path);
            }
            continue;
        }

        let mut chars = line.chars().peekable();
        while chars.peek().is_some() {
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            if chars.peek().is_none() {
                break;
            }
            let quote = match chars.peek() {
                Some('\'' | '"') => chars.next(),
                _ => None,
            };
            let mut token = String::new();
            let mut closed = quote.is_none();
            while let Some(c) = chars.next() {
                if Some(c) == quote {
                    closed = true;
                    break;
                }
                if quote.is_none() && c.is_whitespace() {
                    break;
                }
                if c == '\\' && quote != Some('\'') {
                    token.push(chars.next()?);
                } else if quote.is_none() && (c == '\'' || c == '"') {
                    return None;
                } else {
                    token.push(c);
                }
            }
            if !closed || (quote.is_some() && chars.peek().is_some_and(|c| !c.is_whitespace())) {
                return None;
            }
            let path = image_path(&token)?;
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
    }
    (!paths.is_empty()).then_some(paths)
}

#[cfg(test)]
mod tests {
    use super::pasted_image_paths;
    use std::fs;

    #[test]
    fn parses_quoted_and_unquoted_paths_and_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("screen shot.png");
        let second = dir.path().join("second.JPG");
        fs::write(&first, b"image").unwrap();
        fs::write(&second, b"image").unwrap();
        let input = format!(
            "\"{}\" {}\n'{}'",
            first.display(),
            second.display(),
            first.display()
        );
        assert_eq!(pasted_image_paths(&input), Some(vec![first, second]));
    }

    #[test]
    fn parses_newline_separated_paths_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("one image.png");
        let second = dir.path().join("two image.webp");
        fs::write(&first, b"image").unwrap();
        fs::write(&second, b"image").unwrap();
        let input = format!("{}\n{}", first.display(), second.display());
        assert_eq!(pasted_image_paths(&input), Some(vec![first, second]));
    }

    #[test]
    fn parses_escaped_spaces_and_rejects_mixed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("screen shot.gif");
        fs::write(&image, b"image").unwrap();
        let escaped = image.display().to_string().replace(' ', "\\ ");
        assert_eq!(pasted_image_paths(&escaped), Some(vec![image.clone()]));
        assert_eq!(
            pasted_image_paths(&format!("{}\ncode()", image.display())),
            None
        );
    }

    #[test]
    fn rejects_any_non_image_or_non_path_input() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("image.png");
        let other = dir.path().join("file.txt");
        let directory = dir.path().join("folder.png");
        fs::write(&image, b"image").unwrap();
        fs::write(&other, b"text").unwrap();
        fs::create_dir(&directory).unwrap();
        for input in [
            String::new(),
            "hello world".into(),
            "https://example.com/image.png".into(),
            "file://remote/image.png".into(),
            "relative.png".into(),
            format!("{} {}", image.display(), other.display()),
            format!("{} trailing prose", image.display()),
            format!("\"{}\"code", image.display()),
            format!("\"{}", image.display()),
            dir.path().join("missing.png").display().to_string(),
            directory.display().to_string(),
        ] {
            assert_eq!(pasted_image_paths(&input), None, "{input:?}");
        }
    }
}
