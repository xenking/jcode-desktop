use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

const MAX_FILES: usize = 100_000;
const MAX_ENTRIES: usize = 500_000;

struct IndexedFile {
    path: PathBuf,
    name: String,
    searchable_path: String,
}

/// A bounded snapshot of the visible, non-ignored files in a project.
/// Scanning is synchronous and can be run on a worker thread.
#[derive(Default)]
pub struct FileIndex {
    files: Vec<IndexedFile>,
}

impl FileIndex {
    pub fn scan(root: &Path) -> Self {
        Self::scan_with_hidden(root, false)
    }

    pub fn scan_for_explorer(root: &Path) -> Self {
        Self::scan_with_hidden(root, true)
    }

    fn scan_with_hidden(root: &Path, include_hidden: bool) -> Self {
        let walker = WalkBuilder::new(root)
            .hidden(!include_hidden)
            .follow_links(false)
            .require_git(false)
            .sort_by_file_path(|a, b| a.cmp(b))
            .filter_entry(|entry| {
                entry.depth() == 0
                    || (!hidden_explorer_entry(entry.file_name())
                        && (!entry.file_type().is_some_and(|ty| ty.is_dir())
                            || !matches!(
                                entry.file_name().to_str(),
                                Some(
                                    "target"
                                        | "node_modules"
                                        | "dist"
                                        | "build"
                                        | "__pycache__"
                                        | "vendor"
                                )
                            )))
            })
            .build();
        let mut files = Vec::new();
        for entry in walker.take(MAX_ENTRIES).flatten() {
            if !entry.file_type().is_some_and(|ty| ty.is_file()) {
                continue;
            }
            let Ok(path) = entry.path().strip_prefix(root) else {
                continue;
            };
            let Some(name) = path.file_name() else {
                continue;
            };
            files.push(IndexedFile {
                name: name.to_string_lossy().to_lowercase(),
                searchable_path: path.to_string_lossy().to_lowercase(),
                path: path.to_path_buf(),
            });
            if files.len() == MAX_FILES {
                break;
            }
        }
        Self { files }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<PathBuf> {
        let query = query.to_lowercase();
        self.ranked(limit, |file| {
            if query.is_empty() {
                Some(0)
            } else {
                fuzzy_score(&file.name, &query)
                    .map(|score| score + 10_000)
                    .or_else(|| fuzzy_score(&file.searchable_path, &query))
            }
        })
    }

    pub fn search_terms(&self, query: &str, limit: usize) -> Vec<PathBuf> {
        let query = query.to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        self.ranked(limit, |file| {
            terms
                .iter()
                .all(|term| file.searchable_path.contains(term))
                .then(|| {
                    terms
                        .iter()
                        .filter(|term| file.name.contains(**term))
                        .count() as i64
                })
        })
    }

    fn ranked(&self, limit: usize, score: impl Fn(&IndexedFile) -> Option<i64>) -> Vec<PathBuf> {
        if limit == 0 {
            return Vec::new();
        }
        let mut best = BinaryHeap::new();
        for file in &self.files {
            if let Some(score) = score(file) {
                // The heap root is the worst result: lowest score, then largest path.
                best.push((Reverse(score), &file.path));
                if best.len() > limit {
                    best.pop();
                }
            }
        }
        let mut results: Vec<_> = best.into_iter().collect();
        results.sort_unstable_by(|a, b| b.0.0.cmp(&a.0.0).then_with(|| a.1.cmp(b.1)));
        results.into_iter().map(|(_, path)| path.clone()).collect()
    }
}

pub fn hidden_explorer_entry(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | ".hg" | ".svn" | ".jj" | ".DS_Store")
    )
}

fn fuzzy_score(haystack: &str, needle: &str) -> Option<i64> {
    let mut chars = needle.chars();
    let mut next = chars.next()?;
    let mut score = 0i64;
    let mut previous = None;
    for (index, ch) in haystack.chars().enumerate() {
        if ch != next {
            continue;
        }
        score += 10;
        if previous == Some(index.wrapping_sub(1)) {
            score += 8;
        }
        if index == 0 {
            score += 20;
        }
        previous = Some(index);
        match chars.next() {
            Some(ch) => next = ch,
            None => return Some(score - index as i64),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(root: &Path, path: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn explorer_matches_every_term_and_ranks_file_names_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for path in [
            "src/config/reader.rs",
            "bin/config-reader.rs",
            "config/reader.log",
            ".git/config-reader.rs",
            ".config-reader",
        ] {
            file(root, path);
        }
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let index = FileIndex::scan_for_explorer(root);
        assert_eq!(
            index.search_terms("CONFIG reader", 10),
            [
                ".config-reader",
                "bin/config-reader.rs",
                "src/config/reader.rs"
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn scans_visible_untracked_files_and_respects_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        file(root, ".gitignore");
        fs::write(root.join(".gitignore"), "ignored/\n*.log\n").unwrap();
        for path in [
            "untracked.rs",
            "nested/hello world.rs",
            "nested/Привет.txt",
            "ignored/a.rs",
            "debug.log",
            ".hidden/secret",
            "target/generated.rs",
            "node_modules/pkg/a.js",
        ] {
            file(root, path);
        }
        let index = FileIndex::scan(root);
        assert_eq!(
            index.search("", 20),
            ["nested/hello world.rs", "nested/Привет.txt", "untracked.rs"].map(PathBuf::from)
        );
        assert_eq!(
            index.search("прив", 5),
            [PathBuf::from("nested/Привет.txt")]
        );
        assert_eq!(
            index.search("hello world", 5),
            [PathBuf::from("nested/hello world.rs")]
        );
    }

    #[test]
    fn ranks_basename_then_fuzzy_path_and_obeys_limits() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "src/foobar.rs",
            "other/far_baz.rs",
            "foobar/readme.md",
            "src/FOOBAR.txt",
        ] {
            file(dir.path(), path);
        }
        let index = FileIndex::scan(dir.path());
        assert_eq!(
            index.search("foobar", 2),
            ["src/FOOBAR.txt", "src/foobar.rs"].map(PathBuf::from)
        );
        assert_eq!(index.search("fbr", 1), [PathBuf::from("src/FOOBAR.txt")]);
        assert!(index.search("anything", 0).is_empty());
        assert_eq!(
            index.search("foobar/read", 5),
            [PathBuf::from("foobar/readme.md")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn never_follows_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        file(outside.path(), "secret.txt");
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            root.path().join("link.txt"),
        )
        .unwrap();
        assert!(FileIndex::scan(root.path()).search("", 10).is_empty());
    }
}
