//! Structured, presentation-independent previews of proposed file changes.
//!
//! Tool edits contain snippets, not complete files. Their numbers are relative to
//! each snippet, whereas unified patch numbers are taken from the hunk headers.
//! No filesystem access is performed and a `write` is never assumed to be a create.

use std::collections::HashMap;
use std::ops::Range;

use serde_json::Value;

#[path = "tool_result.rs"]
mod tool_result;

pub fn from_tool_result(name: &str, input: &str, output: &str) -> Option<DiffPreview> {
    tool_result::from_result(name, input, output)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPreview {
    pub files: Vec<DiffFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub kind: String,
    pub hunks: Vec<DiffHunk>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line content without the diff prefix or line terminator.
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    /// UTF-8 byte offsets into `text`, always on character boundaries.
    pub emphasis: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    Meta,
}

impl DiffPreview {
    pub fn counts(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(a, r), file| {
            let (added, removed) = file.counts();
            (a + added, r + removed)
        })
    }
}

impl DiffFile {
    pub fn counts(&self) -> (usize, usize) {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .fold((0, 0), |(added, removed), line| match line.kind {
                LineKind::Added => (added + 1, removed),
                LineKind::Removed => (added, removed + 1),
                _ => (added, removed),
            })
    }

    /// Copyable preview, including file identity, notes and hunk boundaries.
    /// Snippet previews are deliberately not presented as applicable patches.
    pub fn plain_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        if let Some(previous) = &self.previous_path {
            let _ = writeln!(out, "{}: {} -> {}", self.kind, previous, self.path);
        } else {
            let _ = writeln!(out, "{}: {}", self.kind, self.path);
        }
        if let Some(note) = &self.note {
            let _ = writeln!(out, "{note}");
        }
        for hunk in &self.hunks {
            let _ = writeln!(out, "{}", hunk.header);
            for line in &hunk.lines {
                let prefix = match line.kind {
                    LineKind::Context => " ",
                    LineKind::Added => "+",
                    LineKind::Removed => "-",
                    LineKind::Meta => "",
                };
                let _ = writeln!(out, "{prefix}{}", line.text);
            }
        }
        out
    }
}

fn file(path: &str, kind: &str) -> DiffFile {
    DiffFile {
        path: path.to_owned(),
        previous_path: None,
        kind: kind.to_owned(),
        hunks: Vec::new(),
        note: None,
    }
}

fn note(file: &mut DiffFile, text: &str) {
    match &mut file.note {
        Some(existing) if !existing.contains(text) => {
            existing.push(' ');
            existing.push_str(text);
        }
        None => file.note = Some(text.to_owned()),
        _ => {}
    }
}

fn line(kind: LineKind, text: &str, old: Option<usize>, new: Option<usize>) -> DiffLine {
    DiffLine {
        kind,
        text: text.to_owned(),
        old_line: old,
        new_line: new,
        emphasis: None,
    }
}

const SNIPPET_NOTE: &str = "Line numbers are snippet-relative, not file positions.";
const NO_NEWLINE: &str = "\\ No newline at end of file";
const NO_SNIPPET_NEWLINE: &str = "\\ No newline at end of snippet";

/// Recognize desktop file-editing tools, with or without `functions.` prefix.
/// Invalid JSON or missing required arguments does not produce an invented diff.
/// Raw patch arguments may be streaming, so unterminated metadata is buffered.
pub fn from_tool(name: &str, input: &str) -> Option<DiffPreview> {
    parse_tool(name, input, 0)
}

fn parse_tool(name: &str, input: &str, depth: usize) -> Option<DiffPreview> {
    if depth >= 16 {
        return None;
    }
    match serde_json::from_str::<Value>(input) {
        Ok(value) => tool_value(name, &value, depth),
        Err(_)
            if matches!(
                name.trim()
                    .strip_prefix("functions.")
                    .unwrap_or(name.trim()),
                "apply_patch" | "patch"
            ) =>
        {
            raw_patch(input)
        }
        Err(_) => None,
    }
}

fn tool_value(name: &str, value: &Value, depth: usize) -> Option<DiffPreview> {
    if depth >= 16 {
        return None;
    }
    let name = name
        .trim()
        .strip_prefix("functions.")
        .unwrap_or(name.trim());
    if matches!(name, "batch" | "multi_tool_use.parallel") {
        let calls = value
            .get("tool_calls")
            .or_else(|| value.get("tool_uses"))?
            .as_array()?;
        let mut files = Vec::new();
        for call in calls {
            let Some(name) = call
                .get("tool")
                .or_else(|| call.get("recipient_name"))
                .or_else(|| call.get("name"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let arguments = call
                .get("parameters")
                .or_else(|| call.get("arguments"))
                .or_else(|| call.get("input"))
                .unwrap_or(call);
            let preview = if let Some(input) = arguments.as_str() {
                parse_tool(name, input, depth + 1)
            } else {
                tool_value(name, arguments, depth + 1)
            };
            if let Some(preview) = preview {
                files.extend(preview.files);
            }
        }
        return (!files.is_empty()).then_some(DiffPreview { files });
    }
    if !matches!(
        name,
        "edit" | "write" | "multiedit" | "apply_patch" | "patch"
    ) {
        return None;
    }
    if matches!(name, "apply_patch" | "patch") {
        let patch = value.as_str().or_else(|| {
            ["patch_text", "patch", "input"]
                .iter()
                .find_map(|key| value.get(*key).and_then(Value::as_str))
        })?;
        return from_patch(patch);
    }
    let path = value
        .get("file_path")
        .or_else(|| value.get("path"))?
        .as_str()?;
    if path.trim().is_empty() {
        return None;
    }
    let mut result = file(path, if name == "write" { "Write" } else { "Edit" });
    match name {
        "write" => {
            let content = value.get("content")?.as_str()?;
            note(
                &mut result,
                "Proposed write content. Previous file content is unknown.",
            );
            let mut lines = Vec::new();
            for (i, source) in source_lines(content).iter().enumerate() {
                lines.push(line(LineKind::Added, source.text, None, Some(i + 1)));
                if !source.terminated {
                    lines.push(line(LineKind::Meta, NO_NEWLINE, None, None));
                }
            }
            result.hunks.push(DiffHunk {
                header: "@@ Write content (previous content unknown) @@".to_owned(),
                lines,
            });
            if content.is_empty() {
                note(&mut result, "Empty content.");
            }
        }
        "edit" if value.get("edits").is_none() => {
            note(&mut result, SNIPPET_NOTE);
            result.hunks = snippet_diff(
                value.get("old_string")?.as_str()?,
                value.get("new_string")?.as_str()?,
            );
            if value.get("replace_all").and_then(Value::as_bool) == Some(true) {
                note(
                    &mut result,
                    "Replace all matches. Counts show one supplied snippet only.",
                );
            }
        }
        "multiedit" | "edit" => {
            note(&mut result, SNIPPET_NOTE);
            let edits = value.get("edits")?.as_array()?;
            for (i, edit) in edits.iter().enumerate() {
                let mut hunks = snippet_diff(
                    edit.get("old_string")?.as_str()?,
                    edit.get("new_string")?.as_str()?,
                );
                for hunk in &mut hunks {
                    hunk.header = format!("Edit {}: {}", i + 1, hunk.header);
                }
                result.hunks.extend(hunks);
                if edit.get("replace_all").and_then(Value::as_bool) == Some(true) {
                    note(
                        &mut result,
                        "Replace all matches. Counts show supplied snippets only.",
                    );
                }
            }
            note(
                &mut result,
                "Edits are sequential snippets, not a combined file diff.",
            );
        }
        _ => return None,
    }
    if result.hunks.is_empty() {
        note(&mut result, "No changes in the supplied snippets.");
    }
    Some(DiffPreview {
        files: vec![result],
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SourceLine<'a> {
    text: &'a str,
    terminated: bool,
}

fn source_lines(text: &str) -> Vec<SourceLine<'_>> {
    text.split_inclusive('\n')
        .map(|text| SourceLine {
            text: text.strip_suffix('\n').unwrap_or(text),
            terminated: text.ends_with('\n'),
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Op {
    Equal(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// A bounded LCS handles small gaps, with patience anchors for large edits.
/// Total dynamic-programming work per snippet is capped. Large anchorless gaps
/// use bounded lookahead instead of allocating a quadratic matrix.
fn diff_ops<'a>(old: &[SourceLine<'a>], new: &[SourceLine<'a>]) -> Vec<Op> {
    // Comparing long, nearly identical strings in every LCS cell would defeat
    // the work limit. Intern exact lines once, then compare constant-time IDs.
    let mut ids = HashMap::new();
    let mut intern = |source: &SourceLine<'a>| {
        let next = ids.len();
        *ids.entry(*source).or_insert(next)
    };
    let old_ids: Vec<usize> = old.iter().map(&mut intern).collect();
    let new_ids: Vec<usize> = new.iter().map(&mut intern).collect();
    diff_line_ids(&old_ids, &new_ids)
}

fn diff_line_ids(old: &[usize], new: &[usize]) -> Vec<Op> {
    const MAX_CELLS: usize = 300_000;
    let mut budget = MAX_CELLS;
    let mut ops = Vec::with_capacity(old.len().saturating_add(new.len()));
    let mut prefix = 0;
    while prefix < old.len().min(new.len()) && old[prefix] == new[prefix] {
        ops.push(Op::Equal(prefix, prefix));
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len().min(new.len()) - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    if (old_end - prefix + 1)
        .checked_mul(new_end - prefix + 1)
        .is_some_and(|cells| cells <= budget)
    {
        diff_gap(
            old,
            new,
            prefix..old_end,
            prefix..new_end,
            &mut budget,
            &mut ops,
        );
        for offset in 0..suffix {
            ops.push(Op::Equal(old_end + offset, new_end + offset));
        }
        return ops;
    }
    let mut unique: HashMap<usize, (usize, usize, usize, usize)> = HashMap::new();
    for (i, &text) in old.iter().enumerate().take(old_end).skip(prefix) {
        let entry = unique.entry(text).or_insert((0, 0, i, 0));
        entry.0 += 1;
    }
    for (i, &text) in new.iter().enumerate().take(new_end).skip(prefix) {
        let entry = unique.entry(text).or_insert((0, 0, 0, i));
        entry.1 += 1;
        entry.3 = i;
    }
    let pairs: Vec<(usize, usize)> = old
        .iter()
        .enumerate()
        .take(old_end)
        .skip(prefix)
        .filter_map(|(i, text)| {
            let &(a, b, _, j) = unique.get(text)?;
            (a == 1 && b == 1).then_some((i, j))
        })
        .collect();
    // Longest increasing subsequence of the unique lines' new positions.
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; pairs.len()];
    for (i, &(_, j)) in pairs.iter().enumerate() {
        let at = tails.partition_point(|&index| pairs[index].1 < j);
        previous[i] = at.checked_sub(1).map(|at| tails[at]);
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut anchors = Vec::new();
    let mut cursor = tails.last().copied();
    while let Some(i) = cursor {
        anchors.push(pairs[i]);
        cursor = previous[i];
    }
    anchors.reverse();
    let (mut a, mut b) = (prefix, prefix);
    for (i, j) in anchors {
        diff_gap(old, new, a..i, b..j, &mut budget, &mut ops);
        ops.push(Op::Equal(i, j));
        a = i + 1;
        b = j + 1;
    }
    diff_gap(old, new, a..old_end, b..new_end, &mut budget, &mut ops);
    for offset in 0..suffix {
        ops.push(Op::Equal(old_end + offset, new_end + offset));
    }
    ops
}

fn diff_gap(
    old: &[usize],
    new: &[usize],
    mut a: Range<usize>,
    mut b: Range<usize>,
    budget: &mut usize,
    ops: &mut Vec<Op>,
) {
    while !a.is_empty() && !b.is_empty() && old[a.start] == new[b.start] {
        ops.push(Op::Equal(a.start, b.start));
        a.start += 1;
        b.start += 1;
    }
    let mut suffix = 0;
    while suffix < a.len().min(b.len()) && old[a.end - 1 - suffix] == new[b.end - 1 - suffix] {
        suffix += 1;
    }
    a.end -= suffix;
    b.end -= suffix;
    let cells = (a.len() + 1).checked_mul(b.len() + 1);
    if !a.is_empty() && !b.is_empty() && cells.is_some_and(|n| n <= *budget) {
        let cells = cells.unwrap();
        *budget -= cells;
        let width = b.len() + 1;
        let mut lcs = vec![0u32; cells];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i * width + j] = if old[a.start + i] == new[b.start + j] {
                    lcs[(i + 1) * width + j + 1] + 1
                } else {
                    lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && old[a.start + i] == new[b.start + j] {
                ops.push(Op::Equal(a.start + i, b.start + j));
                i += 1;
                j += 1;
            } else if i < a.len()
                && (j == b.len() || lcs[(i + 1) * width + j] >= lcs[i * width + j + 1])
            {
                ops.push(Op::Delete(a.start + i));
                i += 1;
            } else {
                ops.push(Op::Insert(b.start + j));
                j += 1;
            }
        }
    } else {
        fallback_diff(old, new, a.clone(), b.clone(), ops);
    }
    for offset in 0..suffix {
        ops.push(Op::Equal(a.end + offset, b.end + offset));
    }
}

/// Preserve obvious common runs even when every line is repeated and patience
/// anchors are unavailable. The fixed lookahead makes the fallback linear.
fn fallback_diff(
    old: &[usize],
    new: &[usize],
    a: Range<usize>,
    b: Range<usize>,
    ops: &mut Vec<Op>,
) {
    const LOOKAHEAD: usize = 32;
    let (mut i, mut j) = (a.start, b.start);
    while i < a.end && j < b.end {
        if old[i] == new[j] {
            ops.push(Op::Equal(i, j));
            i += 1;
            j += 1;
            continue;
        }
        let removed = (i + 1..a.end.min(i.saturating_add(LOOKAHEAD)))
            .find(|&at| old[at] == new[j])
            .map(|at| at - i);
        let added = (j + 1..b.end.min(j.saturating_add(LOOKAHEAD)))
            .find(|&at| new[at] == old[i])
            .map(|at| at - j);
        match (removed, added) {
            (Some(count), other) if other.is_none_or(|n| count <= n) => {
                ops.extend((i..i + count).map(Op::Delete));
                i += count;
            }
            (_, Some(count)) => {
                ops.extend((j..j + count).map(Op::Insert));
                j += count;
            }
            _ => {
                ops.push(Op::Delete(i));
                ops.push(Op::Insert(j));
                i += 1;
                j += 1;
            }
        }
    }
    ops.extend((i..a.end).map(Op::Delete));
    ops.extend((j..b.end).map(Op::Insert));
}

fn snippet_diff(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
    const CONTEXT: usize = 3;
    let old = source_lines(old_text);
    let new = source_lines(new_text);
    let ops = diff_ops(&old, &new);
    let mut spans: Vec<Range<usize>> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        if !matches!(op, Op::Equal(..)) {
            let span = i.saturating_sub(CONTEXT)..(i + CONTEXT + 1).min(ops.len());
            if let Some(previous) = spans.last_mut().filter(|p| p.end >= span.start) {
                previous.end = span.end;
            } else {
                spans.push(span);
            }
        }
    }
    // Record cursors once. Avoid rescanning the prefix for each separated hunk.
    let mut positions = Vec::with_capacity(ops.len() + 1);
    let (mut a, mut b) = (0, 0);
    positions.push((a, b));
    for op in &ops {
        match op {
            Op::Equal(..) => {
                a += 1;
                b += 1;
            }
            Op::Delete(_) => a += 1,
            Op::Insert(_) => b += 1,
        }
        positions.push((a, b));
    }
    spans.into_iter().map(|span| {
        let (old_before, new_before) = positions[span.start];
        let (old_after, new_after) = positions[span.end];
        let old_count = old_after - old_before;
        let new_count = new_after - new_before;
        let old_start = old_before + usize::from(old_count != 0);
        let new_start = new_before + usize::from(new_count != 0);
        let mut lines = Vec::new();
        for op in &ops[span] {
            let (kind, source, old_line, new_line) = match *op {
                Op::Equal(a, b) => (LineKind::Context, old[a], Some(a + 1), Some(b + 1)),
                Op::Delete(a) => (LineKind::Removed, old[a], Some(a + 1), None),
                Op::Insert(b) => (LineKind::Added, new[b], None, Some(b + 1)),
            };
            lines.push(line(kind, source.text, old_line, new_line));
            if !source.terminated {
                lines.push(line(LineKind::Meta, NO_SNIPPET_NEWLINE, None, None));
            }
        }
        emphasize(&mut lines);
        DiffHunk {
            header: format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@ (snippet-relative lines)"),
            lines,
        }
    }).collect()
}

/// Pair adjacent removed/added blocks without repeatedly scanning entire hunks.
fn emphasize(lines: &mut [DiffLine]) {
    let mut start = 0;
    while start < lines.len() {
        if lines[start].kind == LineKind::Context {
            start += 1;
            continue;
        }
        let mut end = start;
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        while end < lines.len() && lines[end].kind != LineKind::Context {
            match lines[end].kind {
                LineKind::Removed => removed.push(end),
                LineKind::Added => added.push(end),
                _ => {}
            }
            end += 1;
        }
        for (a, b) in replacement_pairs(lines, &removed, &added) {
            let (old, new) = changed_ranges(&lines[a].text, &lines[b].text);
            lines[a].emphasis = old;
            lines[b].emphasis = new;
        }
        start = end;
    }
}

/// Return absolute `(removed_index, added_index)` pairs for one changed block.
/// The supplied indices should follow source order. Pairing never crosses that
/// order, and equal-sized runs retain their stable positional alignment.
/// Unequal runs may skip surplus lines on the longer side, for example a newly
/// inserted comment before a replacement. Work is bounded by fixed lookahead
/// and capped prefix/suffix comparisons, rather than a cross-product matrix.
pub fn replacement_pairs(
    lines: &[DiffLine],
    removed: &[usize],
    added: &[usize],
) -> Vec<(usize, usize)> {
    const LOOKAHEAD: usize = 16;
    // Trim indentation once per line, not once per candidate comparison.
    let indexed = |indices: &[usize], kind| {
        indices
            .iter()
            .filter_map(|&index| {
                let line = lines.get(index).filter(|line| line.kind == kind)?;
                Some((index, line.text.trim_start()))
            })
            .collect::<Vec<_>>()
    };
    let removed = indexed(removed, LineKind::Removed);
    let added = indexed(added, LineKind::Added);
    if removed.len() == added.len() {
        return removed
            .iter()
            .zip(&added)
            .map(|(a, b)| (a.0, b.0))
            .collect();
    }
    let removed_is_shorter = removed.len() < added.len();
    let (shorter, longer) = if removed_is_shorter {
        (&removed, &added)
    } else {
        (&added, &removed)
    };
    let mut result = Vec::with_capacity(shorter.len());
    let mut cursor = 0;
    for (at, &(short_index, short_text)) in shorter.iter().enumerate() {
        // Leave at least one candidate for every remaining shorter-side line.
        let surplus = longer.len() - cursor - (shorter.len() - at);
        let last = cursor + surplus.min(LOOKAHEAD);
        let mut best = cursor;
        let mut best_score = replacement_similarity(short_text, longer[cursor].1);
        for (candidate, line) in longer.iter().enumerate().take(last + 1).skip(cursor + 1) {
            let score = replacement_similarity(short_text, line.1);
            // Nearest candidate wins ties, including wholly unrelated lines.
            if score > best_score {
                best = candidate;
                best_score = score;
            }
        }
        let long_index = longer[best].0;
        result.push(if removed_is_shorter {
            (short_index, long_index)
        } else {
            (long_index, short_index)
        });
        cursor = best + 1;
    }
    result
}

fn replacement_similarity(old: &str, new: &str) -> usize {
    const EDGE_CHARS: usize = 128;
    let mut prefix_bytes = 0;
    let mut prefix_chars = 0;
    for (a, b) in old.chars().zip(new.chars()).take(EDGE_CHARS) {
        if a != b {
            break;
        }
        prefix_bytes += a.len_utf8();
        prefix_chars += 1;
    }
    // Slice on matched character boundaries so prefix and suffix cannot count
    // the same character twice. Prefix gets extra weight over common closing
    // punctuation, which alone is weak evidence of an actual replacement.
    let suffix_chars = old[prefix_bytes..]
        .chars()
        .rev()
        .zip(new[prefix_bytes..].chars().rev())
        .take(EDGE_CHARS)
        .take_while(|(a, b)| a == b)
        .count();
    prefix_chars * 2 + suffix_chars
}

fn changed_ranges(old: &str, new: &str) -> (Option<Range<usize>>, Option<Range<usize>>) {
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    (
        (prefix < old_end).then_some(prefix..old_end),
        (prefix < new_end).then_some(prefix..new_end),
    )
}

/// Parse Codex patches and ordinary unified diffs, preserving all files/hunks.
/// Incomplete or malformed hunks retain useful content with an explicit note.
/// Unlike raw input to `from_tool`, this argument is considered complete, so a
/// final metadata line does not need a trailing newline.
pub fn from_patch(patch: &str) -> Option<DiffPreview> {
    parse_patch(patch, false)
}

fn raw_patch(patch: &str) -> Option<DiffPreview> {
    let patch = patch.trim_start();
    // Incomplete JSON must not be reinterpreted as freeform patch content.
    if !(patch.starts_with("*** ") || patch.starts_with("diff --git ") || patch.starts_with("--- "))
    {
        return None;
    }
    parse_patch(patch, !patch.ends_with('\n'))
}

fn parse_patch(patch: &str, partial_last_line: bool) -> Option<DiffPreview> {
    if patch.lines().any(|l| {
        matches!(l.trim_end_matches('\r'), "*** Begin Patch")
            || l.starts_with("*** Update File: ")
            || l.starts_with("*** Add File: ")
            || l.starts_with("*** Delete File: ")
    }) {
        codex_patch(patch, partial_last_line)
    } else {
        unified_patch(patch, partial_last_line)
    }
}

fn finish_file(current: &mut Option<DiffFile>, files: &mut Vec<DiffFile>) {
    if let Some(mut file) = current.take() {
        for hunk in &mut file.hunks {
            emphasize(&mut hunk.lines);
        }
        files.push(file);
    }
}

fn codex_patch(patch: &str, partial_last_line: bool) -> Option<DiffPreview> {
    let mut files = Vec::new();
    let mut current = None;
    let (mut old, mut new) = (1usize, 1usize);
    let lines: Vec<_> = patch.lines().collect();
    for (index, raw) in lines.iter().enumerate() {
        let raw = raw.trim_end_matches('\r');
        if partial_last_line
            && index + 1 == lines.len()
            && (raw.starts_with("***") || raw.starts_with("@@"))
        {
            break;
        }
        let declaration = [
            ("*** Update File: ", "Edit"),
            ("*** Add File: ", "Add"),
            ("*** Delete File: ", "Delete"),
        ]
        .iter()
        .find_map(|(prefix, kind)| raw.strip_prefix(prefix).map(|p| (p, *kind)));
        if let Some((path, kind)) = declaration {
            finish_file(&mut current, &mut files);
            if path.trim().is_empty() {
                continue;
            }
            let mut next = file(path, kind);
            if kind == "Edit" {
                note(&mut next, SNIPPET_NOTE);
            } else if kind == "Delete" {
                note(
                    &mut next,
                    "File deletion. Previous content was not supplied.",
                );
            }
            current = Some(next);
            old = 1;
            new = 1;
            continue;
        }
        if raw == "*** End Patch" {
            finish_file(&mut current, &mut files);
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };
        if let Some(path) = raw.strip_prefix("*** Move to: ") {
            if !path.trim().is_empty() {
                file.previous_path = Some(std::mem::replace(&mut file.path, path.to_owned()));
                file.kind = "Rename".to_owned();
            } else {
                note(file, "Malformed move destination.");
            }
        } else if raw.starts_with("@@") {
            old = 1;
            new = 1;
            file.hunks.push(DiffHunk {
                header: format!(
                    "{raw} (snippet-relative lines, hunk {})",
                    file.hunks.len() + 1
                ),
                lines: Vec::new(),
            });
        } else if raw == "*** End of File" || raw == NO_NEWLINE {
            if let Some(hunk) = file.hunks.last_mut() {
                hunk.lines.push(line(LineKind::Meta, raw, None, None));
            }
        } else {
            let parsed = match raw.as_bytes().first() {
                Some(b'+') => Some((LineKind::Added, &raw[1..])),
                Some(b'-') => Some((LineKind::Removed, &raw[1..])),
                Some(b' ') => Some((LineKind::Context, &raw[1..])),
                None if !file.hunks.is_empty() && file.kind != "Add" => {
                    Some((LineKind::Context, ""))
                }
                _ => None,
            };
            if let Some((kind, text)) = parsed {
                if file.hunks.is_empty() {
                    file.hunks.push(DiffHunk {
                        header: if file.kind == "Add" {
                            "@@ New file, line 1 @@".to_owned()
                        } else {
                            "@@ (snippet-relative lines, hunk 1) @@".to_owned()
                        },
                        lines: Vec::new(),
                    });
                }
                let a = matches!(kind, LineKind::Context | LineKind::Removed).then_some(old);
                let b = matches!(kind, LineKind::Context | LineKind::Added).then_some(new);
                old = old.saturating_add(usize::from(a.is_some()));
                new = new.saturating_add(usize::from(b.is_some()));
                file.hunks
                    .last_mut()
                    .unwrap()
                    .lines
                    .push(line(kind, text, a, b));
            } else if !raw.is_empty() {
                note(file, "Unrecognized patch content was omitted.");
            }
        }
    }
    finish_file(&mut current, &mut files);
    (!files.is_empty()).then_some(DiffPreview { files })
}

#[derive(Clone, Copy)]
struct HunkPosition {
    old: usize,
    new: usize,
    old_left: usize,
    new_left: usize,
}

fn parse_range(text: &str, sign: char) -> Option<(usize, usize)> {
    let text = text.strip_prefix(sign)?;
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    if !start.bytes().all(|b| b.is_ascii_digit()) || !count.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (start, count) = (start.parse::<usize>().ok()?, count.parse::<usize>().ok()?);
    if (start == 0 && count != 0) || start.checked_add(count).is_none() {
        return None;
    }
    Some((start, count))
}

fn hunk_position(header: &str) -> Option<HunkPosition> {
    let mut parts = header.strip_prefix("@@ ")?.split_whitespace();
    let (old, old_left) = parse_range(parts.next()?, '-')?;
    let (new, new_left) = parse_range(parts.next()?, '+')?;
    if parts.next()? != "@@" {
        return None;
    }
    Some(HunkPosition {
        old,
        new,
        old_left,
        new_left,
    })
}

fn incomplete_hunk(file: &mut DiffFile, position: &mut Option<HunkPosition>) {
    if position
        .take()
        .is_some_and(|p| p.old_left != 0 || p.new_left != 0)
    {
        note(file, "Incomplete or malformed unified hunk.");
    }
}

fn unified_patch(patch: &str, partial_last_line: bool) -> Option<DiffPreview> {
    let lines: Vec<&str> = patch.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut files = Vec::new();
    let mut current: Option<DiffFile> = None;
    let mut position: Option<HunkPosition> = None;
    let mut saw_headers = false;
    let mut invalid_hunk = false;
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        // Inside a declared hunk, --- and +++ may be literal changed lines.
        if let (Some(file), Some(p)) = (current.as_mut(), position.as_mut()) {
            if raw == NO_NEWLINE {
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.lines.push(line(LineKind::Meta, raw, None, None));
                }
                i += 1;
                continue;
            }
            let kind = match raw.as_bytes().first() {
                Some(b' ') if p.old_left > 0 && p.new_left > 0 => Some(LineKind::Context),
                Some(b'-') if p.old_left > 0 => Some(LineKind::Removed),
                Some(b'+') if p.new_left > 0 => Some(LineKind::Added),
                None if p.old_left > 0 && p.new_left > 0 => Some(LineKind::Context),
                _ => None,
            };
            if let Some(kind) = kind {
                let a = matches!(kind, LineKind::Context | LineKind::Removed).then_some(p.old);
                let b = matches!(kind, LineKind::Context | LineKind::Added).then_some(p.new);
                if a.is_some() {
                    p.old += 1;
                    p.old_left -= 1;
                }
                if b.is_some() {
                    p.new += 1;
                    p.new_left -= 1;
                }
                file.hunks.last_mut().unwrap().lines.push(line(
                    kind,
                    raw.get(1..).unwrap_or(""),
                    a,
                    b,
                ));
                i += 1;
                continue;
            }
            invalid_hunk |= p.old_left != 0 || p.new_left != 0;
            incomplete_hunk(file, &mut position);
        }
        // Body text streams immediately. Metadata waits for its line ending.
        if partial_last_line && i + 1 == lines.len() {
            break;
        }
        if invalid_hunk && !raw.starts_with("diff --git ") {
            if let Some(file) = current.as_mut() {
                unpositioned_line(file, raw);
            }
            i += 1;
            continue;
        }
        if let Some(paths) = raw.strip_prefix("diff --git ") {
            invalid_hunk = false;
            if let Some(file) = current.as_mut() {
                incomplete_hunk(file, &mut position);
            }
            finish_file(&mut current, &mut files);
            if let Some((a, b)) = git_paths(paths) {
                let mut next = file(&b, "Edit");
                if a != b {
                    next.previous_path = Some(a);
                    next.kind = "Rename".to_owned();
                }
                current = Some(next);
            }
            saw_headers = false;
        } else if let Some(a) = raw
            .strip_prefix("--- ")
            .filter(|_| lines.get(i + 1).is_some_and(|l| l.starts_with("+++ ")))
        {
            if partial_last_line && i + 2 == lines.len() {
                break;
            }
            if saw_headers {
                finish_file(&mut current, &mut files);
            }
            let mut a = header_path(a);
            let mut b = header_path(&lines[i + 1][4..]);
            // Strip only a conventional paired Git prefix, not a real `a/`
            // directory appearing identically on both sides of a plain diff.
            if (a.starts_with("a/") && b.starts_with("b/"))
                || (a == "/dev/null" && b.starts_with("b/"))
                || (b == "/dev/null" && a.starts_with("a/"))
            {
                a = a.strip_prefix("a/").unwrap_or(&a).to_owned();
                b = b.strip_prefix("b/").unwrap_or(&b).to_owned();
            }
            if !(a.is_empty() || b.is_empty() || a == "/dev/null" && b == "/dev/null") {
                let path = if b == "/dev/null" { &a } else { &b };
                let next = current.get_or_insert_with(|| file(path, "Edit"));
                next.path = path.to_owned();
                if a == "/dev/null" {
                    next.kind = "Add".to_owned();
                    next.previous_path = None;
                } else if b == "/dev/null" {
                    next.kind = "Delete".to_owned();
                    next.previous_path = None;
                } else if a != b {
                    next.kind = "Rename".to_owned();
                    next.previous_path = Some(a);
                }
                saw_headers = true;
            }
            i += 1;
        } else if let Some(file) = current.as_mut() {
            if raw.starts_with("@@") {
                position = hunk_position(raw);
                if position.is_none() {
                    invalid_hunk = true;
                    note(
                        file,
                        "Malformed unified hunk header. Line positions are unavailable.",
                    );
                }
                file.hunks.push(DiffHunk {
                    header: raw.to_owned(),
                    lines: Vec::new(),
                });
            } else if let Some(path) = raw.strip_prefix("rename from ") {
                file.previous_path = Some(unquote_path(path));
                file.kind = "Rename".to_owned();
            } else if let Some(path) = raw.strip_prefix("rename to ") {
                file.path = unquote_path(path);
                file.kind = "Rename".to_owned();
            } else if raw.starts_with("new file mode ") {
                file.kind = "Add".to_owned();
            } else if raw.starts_with("deleted file mode ") {
                file.kind = "Delete".to_owned();
            } else if raw.starts_with("Binary files ") || raw == "GIT binary patch" {
                note(file, "Binary file change. Text preview unavailable.");
            } else if raw.starts_with("old mode ") || raw.starts_with("new mode ") {
                note(file, raw);
            } else if raw.starts_with(['+', '-', ' ']) && !file.hunks.is_empty() {
                // Keep malformed/truncated preview content, but never invent positions.
                note(file, "Incomplete or malformed unified hunk.");
                invalid_hunk = true;
                unpositioned_line(file, raw);
            } else if raw == NO_NEWLINE {
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.lines.push(line(LineKind::Meta, raw, None, None));
                }
            } else if !raw.is_empty() && !file.hunks.is_empty() {
                invalid_hunk = true;
                note(file, "Incomplete or malformed unified hunk.");
            }
        }
        i += 1;
    }
    if let Some(file) = current.as_mut() {
        incomplete_hunk(file, &mut position);
    }
    finish_file(&mut current, &mut files);
    (!files.is_empty()).then_some(DiffPreview { files })
}

fn unpositioned_line(file: &mut DiffFile, raw: &str) {
    let Some(hunk) = file.hunks.last_mut() else {
        return;
    };
    let kind = match raw.as_bytes().first() {
        Some(b'+') => LineKind::Added,
        Some(b'-') => LineKind::Removed,
        Some(b' ') => LineKind::Context,
        _ => return,
    };
    hunk.lines.push(line(kind, &raw[1..], None, None));
}

fn header_path(text: &str) -> String {
    // Unified timestamps are separated by a tab. Spaces belong to the path.
    if text.starts_with('"') {
        quoted_token(text)
            .map(|(p, _)| p)
            .unwrap_or_else(|| text.to_owned())
    } else {
        text.split('\t').next().unwrap_or(text).to_owned()
    }
}

fn unquote_path(text: &str) -> String {
    quoted_token(text)
        .map(|(path, _)| path)
        .unwrap_or_else(|| text.to_owned())
}

/// Git's quoted paths use C escapes and octal UTF-8 bytes.
fn quoted_token(text: &str) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), i + 1)),
            b'\\' => {
                i += 1;
                let byte = *bytes.get(i)?;
                if (b'0'..=b'7').contains(&byte) {
                    let mut value = 0u16;
                    let mut digits = 0;
                    while digits < 3 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                        value = value * 8 + (bytes[i] - b'0') as u16;
                        i += 1;
                        digits += 1;
                    }
                    out.push(u8::try_from(value).ok()?);
                    continue;
                }
                out.push(match byte {
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'r' => b'\r',
                    b'b' => 8,
                    b'f' => 12,
                    b'v' => 11,
                    b'a' => 7,
                    other => other,
                });
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    None
}

fn git_paths(text: &str) -> Option<(String, String)> {
    // Equal unquoted paths have a unique midpoint even when the filename
    // itself contains ` b/`. Checking that split once avoids quadratic scans.
    if !text.starts_with('"') {
        let middle = text.len().saturating_sub(1) / 2;
        if let (Some(a), Some(b)) = (text.get(..middle), text.get(middle..))
            && let (Some(a), Some(b)) = (a.strip_prefix("a/"), b.strip_prefix(" b/"))
                && !a.is_empty() && a == b {
                    return Some((a.to_owned(), b.to_owned()));
                }
    }
    let (a, rest) = if text.starts_with('"') {
        let (a, end) = quoted_token(text)?;
        (a, text.get(end..)?.trim_start())
    } else if let Some(at) = text.find(" \"b/").or_else(|| text.find(" b/")) {
        (text[..at].to_owned(), &text[at + 1..])
    } else {
        let (a, b) = text.split_once(' ')?;
        (a.to_owned(), b)
    };
    let b = unquote_path(rest);
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((
        a.strip_prefix("a/").unwrap_or(&a).to_owned(),
        b.strip_prefix("b/").unwrap_or(&b).to_owned(),
    ))
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
