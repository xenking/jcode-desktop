use super::*;
use serde_json::json;

fn edit(old: &str, new: &str) -> DiffFile {
    from_tool(
        "edit",
        &json!({"file_path":"src/test.rs", "old_string":old, "new_string":new}).to_string(),
    )
    .unwrap()
    .files
    .remove(0)
}

fn all_lines(file: &DiffFile) -> Vec<&DiffLine> {
    file.hunks.iter().flat_map(|h| &h.lines).collect()
}

#[test]
fn edit_keeps_common_lines_as_context() {
    let file = edit("start\nold\ncommon\nend\n", "start\nnew\ncommon\nend\n");
    assert_eq!(file.counts(), (1, 1));
    assert_eq!(file.hunks.len(), 1);
    assert!(file.note.as_deref().unwrap().contains("snippet-relative"));
    assert_eq!(
        file.hunks[0].header,
        "@@ -1,4 +1,4 @@ (snippet-relative lines)"
    );
    let lines = all_lines(&file);
    assert_eq!(
        lines.iter().filter(|l| l.kind == LineKind::Context).count(),
        3
    );
    assert_eq!((lines[1].old_line, lines[1].new_line), (Some(2), None));
    assert_eq!((lines[2].old_line, lines[2].new_line), (None, Some(2)));
}

#[test]
fn edit_separates_distant_changes_and_clips_context() {
    let old = (1..=40).map(|n| format!("line {n}\n")).collect::<String>();
    let new = old
        .replace("line 5\n", "changed five\n")
        .replace("line 35\n", "changed thirty five\n");
    let file = edit(&old, &new);
    assert_eq!(file.counts(), (2, 2));
    assert_eq!(file.hunks.len(), 2);
    assert!(file.hunks[0].header.starts_with("@@ -2,7 +2,7 @@"));
    assert!(file.hunks[1].header.starts_with("@@ -32,7 +32,7 @@"));
    assert!(!file.plain_text().contains("line 20\n"));
}

#[test]
fn edit_merges_overlapping_context() {
    let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let file = edit(old, &old.replace("b\n", "B\n").replace("g\n", "G\n"));
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.counts(), (2, 2));
    assert_eq!(all_lines(&file).len(), 10);
}

#[test]
fn edit_insert_delete_and_empty_snippets() {
    let inserted = edit("", "\nhello\n");
    assert_eq!(inserted.counts(), (2, 0));
    assert_eq!(
        inserted.hunks[0].header,
        "@@ -0,0 +1,2 @@ (snippet-relative lines)"
    );
    assert_eq!(inserted.hunks[0].lines[0].text, "");
    let deleted = edit("hello\n\n", "");
    assert_eq!(deleted.counts(), (0, 2));
    assert!(deleted.hunks[0].header.contains("+0,0"));
    for text in ["", "a", "a\n", "\n\n"] {
        let same = edit(text, text);
        assert!(same.hunks.is_empty());
        assert_eq!(same.counts(), (0, 0));
        assert!(same.note.as_deref().unwrap().contains("No changes"));
    }
}

#[test]
fn newline_only_edit_is_visible_and_markers_do_not_count() {
    let file = edit("same", "same\n");
    assert_eq!(file.counts(), (1, 1));
    let lines = all_lines(&file);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1].kind, LineKind::Meta);
    assert_eq!(lines[1].text, NO_SNIPPET_NEWLINE);
    assert_eq!((lines[1].old_line, lines[1].new_line), (None, None));
    assert!(lines.iter().all(|l| l.emphasis.is_none()));
    let file = edit("old", "new");
    assert_eq!(
        all_lines(&file)
            .iter()
            .filter(|l| l.kind == LineKind::Meta)
            .count(),
        2
    );
}

#[test]
fn multiedit_has_distinct_relative_hunks_and_replace_all_note() {
    let preview = from_tool(
        "functions.multiedit",
        &json!({"file_path":"a", "edits":[
            {"old_string":"old\n", "new_string":"new\n"},
            {"old_string":"x\n", "new_string":"y\nz\n", "replace_all":true}
        ]})
        .to_string(),
    )
    .unwrap();
    let file = &preview.files[0];
    assert_eq!(file.hunks.len(), 2);
    assert_eq!(preview.counts(), (3, 2));
    assert!(file.hunks[0].header.starts_with("Edit 1: "));
    assert!(file.hunks[1].header.starts_with("Edit 2: "));
    assert_eq!(file.hunks[1].lines[0].old_line, Some(1));
    assert!(file.note.as_deref().unwrap().contains("sequential"));
    assert!(file.note.as_deref().unwrap().contains("Replace all"));
}

#[test]
fn edit_with_edits_array_matches_multiedit_preview() {
    let input = json!({"file_path":"a", "edits":[
        {"old_string":"old\n", "new_string":"new\n"},
        {"old_string":"x\n", "new_string":"y\n"}
    ]})
    .to_string();
    let merged = from_tool("edit", &input).unwrap();
    let legacy = from_tool("multiedit", &input).unwrap();
    assert_eq!(merged.counts(), legacy.counts());
    assert_eq!(merged.files[0].hunks.len(), 2);
    assert!(merged.files[0].hunks[1].header.starts_with("Edit 2: "));
}

#[test]
fn edit_replace_all_does_not_claim_total_file_counts() {
    let preview = from_tool(
        "edit",
        r#"{"file_path":"a","old_string":"x","new_string":"y","replace_all":true}"#,
    )
    .unwrap();
    assert!(
        preview.files[0]
            .note
            .as_deref()
            .unwrap()
            .contains("one supplied snippet")
    );
}

#[test]
fn write_is_not_mislabeled_as_create() {
    for content in ["", "\n", "hello\n\n", "hello"] {
        let preview = from_tool(
            "functions.write",
            &json!({"file_path":"existing.txt", "content":content}).to_string(),
        )
        .unwrap();
        let file = &preview.files[0];
        assert_eq!(file.kind, "Write");
        assert!(
            file.note
                .as_deref()
                .unwrap()
                .contains("Previous file content is unknown")
        );
        assert_eq!(preview.counts(), (source_lines(content).len(), 0));
        assert!(all_lines(file).iter().all(|l| l.old_line.is_none()));
        assert!(!file.plain_text().contains("New file"));
    }
}

#[test]
fn malformed_tool_inputs_are_rejected() {
    for (name, input) in [
        ("edit", "not json"),
        ("edit", "null"),
        ("edit", "{}"),
        (
            "edit",
            r#"{"file_path":"a","old_string":3,"new_string":"x"}"#,
        ),
        ("write", r#"{"file_path":" ","content":"x"}"#),
        ("multiedit", r#"{"file_path":"a","edits":[{}]}"#),
        ("patch", "{}"),
        ("functions.read", "{}"),
        (
            "other.edit",
            r#"{"file_path":"a","old_string":"x","new_string":"y"}"#,
        ),
    ] {
        assert!(from_tool(name, input).is_none(), "{name}: {input}");
    }
    assert!(from_patch("").is_none());
    assert!(from_patch("plain text\n@@ -1 +1 @@\n-a\n+b").is_none());
    assert!(from_patch("*** Begin Patch\n*** End Patch").is_none());
}

#[test]
fn batch_collects_supported_files_and_skips_unrelated_calls() {
    let input = json!({"tool_calls":[
        {"tool":"functions.edit", "file_path":"a", "old_string":"x\n", "new_string":"y\n"},
        {"tool":"read", "file_path":"ignored"},
        {"tool":"write", "file_path":"b", "content":"hello\n"},
        {"tool":"edit", "file_path":"malformed"},
        {"tool":"patch", "patch_text":"*** Delete File: c"}
    ]});
    let preview = from_tool("functions.batch", &input.to_string()).unwrap();
    assert_eq!(
        preview
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert_eq!(preview.counts(), (2, 1));
    assert!(from_tool("batch", r#"{"tool_calls":[{"tool":"read"}]}"#).is_none());
}

#[test]
fn parallel_recurses_through_nested_batches_and_string_arguments() {
    let input = json!({"tool_uses":[
        {"recipient_name":"functions.batch", "parameters":{"tool_calls":[
            {"tool":"functions.edit", "arguments":{"file_path":"a", "old_string":"x", "new_string":"y"}},
            {"tool":"functions.write", "input":json!({"file_path":"b", "content":"written\n"}).to_string()}
        ]}},
        {"recipient_name":"functions.apply_patch", "parameters":{"patch_text":"*** Add File: c\n+new\n"}},
        {"name":"edit", "arguments":{"file_path":"d", "old_string":"x", "new_string":"z"}}
    ]});
    let preview = from_tool("multi_tool_use.parallel", &input.to_string()).unwrap();
    assert_eq!(preview.files.len(), 4);
    assert_eq!(preview.counts(), (4, 2));
}

#[test]
fn recursive_batches_are_bounded_without_discarding_shallow_siblings() {
    let mut nested = json!({"tool":"write", "file_path":"deep", "content":"x"});
    for _ in 0..20 {
        nested = json!({"tool":"batch", "tool_calls":[nested]});
    }
    let input =
        json!({"tool_calls":[nested, {"tool":"write", "file_path":"shallow", "content":"y"}]});
    let preview = from_tool("batch", &input.to_string()).unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "shallow");
}

#[test]
fn patch_tools_accept_wrapped_and_raw_formats() {
    let patch = "*** Begin Patch\n*** Add File: x\n+hi\n*** End Patch";
    for name in [
        "apply_patch",
        "patch",
        "functions.apply_patch",
        "functions.patch",
    ] {
        for input in [
            patch.to_owned(),
            json!(patch).to_string(),
            json!({"patch_text":patch}).to_string(),
            json!({"patch":patch}).to_string(),
            json!({"input":patch}).to_string(),
        ] {
            assert_eq!(from_tool(name, &input).unwrap().counts(), (1, 0));
        }
    }
}

#[test]
fn codex_multiple_files_add_delete_and_move() {
    let patch = "*** Begin Patch\n*** Add File: new file.txt\n+first\n+\n+third\n*** Delete File: gone.txt\n*** Update File: before.rs\n*** Move to: after.rs\n@@ fn first()\n context\n-old\n+new\n@@ fn second()\n-x\n+y\n*** End of File\n*** End Patch";
    let preview = from_patch(patch).unwrap();
    assert_eq!(preview.files.len(), 3);
    assert_eq!(preview.counts(), (5, 2));
    let add = &preview.files[0];
    assert_eq!((&*add.path, &*add.kind), ("new file.txt", "Add"));
    assert_eq!(add.hunks[0].lines[1].text, "");
    assert_eq!(add.hunks[0].lines[2].new_line, Some(3));
    let delete = &preview.files[1];
    assert_eq!(delete.kind, "Delete");
    assert_eq!(delete.counts(), (0, 0));
    assert!(delete.note.as_deref().unwrap().contains("not supplied"));
    let moved = &preview.files[2];
    assert_eq!((&*moved.path, &*moved.kind), ("after.rs", "Rename"));
    assert_eq!(moved.previous_path.as_deref(), Some("before.rs"));
    assert_eq!(moved.hunks.len(), 2);
    assert_eq!(moved.hunks[1].lines[0].old_line, Some(1));
    assert!(
        moved
            .hunks
            .iter()
            .all(|h| h.header.contains("snippet-relative"))
    );
    assert_eq!(moved.hunks[1].lines[2].kind, LineKind::Meta);
}

#[test]
fn codex_no_header_update_still_has_context_and_relative_label() {
    let preview = from_patch("*** Update File: x\n before\n-old\n+new\n after\n").unwrap();
    let file = &preview.files[0];
    assert_eq!(file.counts(), (1, 1));
    assert!(file.hunks[0].header.contains("snippet-relative"));
    assert_eq!(file.hunks[0].lines[3].old_line, Some(3));
    assert_eq!(file.hunks[0].lines[3].new_line, Some(3));
}

#[test]
fn codex_malformed_empty_path_and_move_are_safe() {
    let preview = from_patch("*** Begin Patch\n*** Add File: \n+ignored\n*** Update File: x\n*** Move to: \nunrecognized\n@@\n-old\n+new\n").unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "x");
    assert_eq!(preview.files[0].counts(), (1, 1));
    assert!(
        preview.files[0]
            .note
            .as_deref()
            .unwrap()
            .contains("Malformed")
    );
}

#[test]
fn unified_actual_line_numbers_and_multiple_hunks() {
    let preview = from_patch("diff --git a/src/a.rs b/src/a.rs\nindex aaa..bbb 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -40,3 +50,4 @@ fn demo()\n first\n-old\n+new\n+extra\n last\n@@ -100 +111 @@\n-a\n+b\n").unwrap();
    let file = &preview.files[0];
    assert_eq!(file.path, "src/a.rs");
    assert_eq!(file.counts(), (3, 2));
    assert_eq!(file.note, None);
    assert_eq!(file.hunks[0].header, "@@ -40,3 +50,4 @@ fn demo()");
    let first = &file.hunks[0].lines;
    assert_eq!((first[0].old_line, first[0].new_line), (Some(40), Some(50)));
    assert_eq!((first[4].old_line, first[4].new_line), (Some(42), Some(53)));
    assert_eq!(file.hunks[1].lines[0].old_line, Some(100));
    assert_eq!(file.hunks[1].lines[1].new_line, Some(111));
}

#[test]
fn unified_create_delete_rename_without_git_headers() {
    let preview = from_patch("--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+new\n+\n--- a/deleted.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-old\n-\n--- a/old.txt\n+++ b/renamed.txt\n@@ -5 +8 @@\n-a\n+b\n").unwrap();
    assert_eq!(preview.files.len(), 3);
    assert_eq!(preview.counts(), (3, 3));
    assert_eq!(
        (&*preview.files[0].path, &*preview.files[0].kind),
        ("new.txt", "Add")
    );
    assert_eq!(
        (&*preview.files[1].path, &*preview.files[1].kind),
        ("deleted.txt", "Delete")
    );
    assert_eq!(preview.files[2].previous_path.as_deref(), Some("old.txt"));
    assert_eq!(preview.files[2].kind, "Rename");
    assert!(preview.files.iter().all(|f| f.note.is_none()));
}

#[test]
fn unified_rename_only_empty_create_delete_and_modes() {
    let preview = from_patch("diff --git a/old name b/new name\nsimilarity index 100%\nrename from old name\nrename to new name\ndiff --git a/empty b/empty\nnew file mode 100644\nindex 0000000..e69de29\ndiff --git a/gone b/gone\ndeleted file mode 100644\ndiff --git a/executable b/executable\nold mode 100644\nnew mode 100755\n").unwrap();
    assert_eq!(preview.files.len(), 4);
    assert_eq!(preview.counts(), (0, 0));
    assert_eq!(preview.files[0].path, "new name");
    assert_eq!(preview.files[0].previous_path.as_deref(), Some("old name"));
    assert_eq!(preview.files[1].kind, "Add");
    assert_eq!(preview.files[2].kind, "Delete");
    assert!(preview.files[3].note.as_deref().unwrap().contains("100755"));
}

#[test]
fn unified_blank_context_and_no_newline_markers() {
    let file = from_patch("--- a/x\n+++ b/x\n@@ -7,2 +9,2 @@\n \n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n").unwrap().files.remove(0);
    assert_eq!(file.counts(), (1, 1));
    assert_eq!(file.hunks[0].lines[0].text, "");
    assert_eq!(file.hunks[0].lines[0].kind, LineKind::Context);
    assert_eq!(file.hunks[0].lines[3].new_line, Some(10));
    assert_eq!(file.note, None);
    assert_eq!(file.hunks[0].lines[4].kind, LineKind::Meta);
}

#[test]
fn unified_header_like_content_is_not_a_file_boundary() {
    let preview =
        from_patch("--- a/x\n+++ b/x\n@@ -1 +1 @@\n--- a/not-a-file\n+++ b/not-a-file\n").unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "x");
    assert_eq!(preview.counts(), (1, 1));
    assert_eq!(preview.files[0].hunks[0].lines[0].text, "-- a/not-a-file");
}

#[test]
fn unified_quoted_paths_unicode_octal_and_timestamps() {
    let preview = from_patch("diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n--- \"a/caf\\303\\251.txt\"\t2025-01-01\n+++ \"b/caf\\303\\251.txt\"\t2025-01-02\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
    assert_eq!(preview.files[0].path, "café.txt");
    let preview = from_patch("--- a/file with spaces\told timestamp\n+++ b/file with spaces\tnew timestamp\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
    assert_eq!(preview.files[0].path, "file with spaces");
    assert_eq!(
        quoted_token("\"a\\t\\\"b\" rest"),
        Some(("a\t\"b".to_owned(), 8))
    );
}

#[test]
fn unified_binary_content_is_not_made_into_fake_lines() {
    let preview = from_patch("diff --git a/a.png b/a.png\nindex abc..def 100644\nBinary files a/a.png and b/a.png differ\ndiff --git a/b.png b/b.png\nGIT binary patch\nliteral 4\nAcme\n").unwrap();
    assert_eq!(preview.files.len(), 2);
    assert_eq!(preview.counts(), (0, 0));
    assert!(
        preview
            .files
            .iter()
            .all(|f| f.note.as_deref().unwrap().contains("Binary"))
    );
}

#[test]
fn malformed_unified_header_has_no_invented_positions() {
    for header in [
        "@@ nonsense @@",
        "@@ -0 +1 @@",
        "@@ -1,2 +1,no @@",
        "@@ -18446744073709551615,2 +1 @@",
        "@@ -1 +1 missing",
    ] {
        let preview = from_patch(&format!("--- a/x\n+++ b/x\n{header}\n-old\n+new\n")).unwrap();
        let file = &preview.files[0];
        assert_eq!(file.counts(), (1, 1));
        assert!(file.note.as_deref().unwrap().contains("Malformed"));
        assert!(
            all_lines(file)
                .iter()
                .all(|l| l.old_line.is_none() && l.new_line.is_none())
        );
    }
}

#[test]
fn truncated_unified_hunk_is_explicit_and_next_file_survives() {
    let preview = from_patch("--- a/x\n+++ b/x\n@@ -1,10 +1,10 @@\n-old\n+new\ndiff --git a/y b/y\n--- a/y\n+++ b/y\n@@ -10 +20 @@\n-a\n+b\n").unwrap();
    assert_eq!(preview.files.len(), 2);
    assert!(
        preview.files[0]
            .note
            .as_deref()
            .unwrap()
            .contains("Incomplete")
    );
    assert_eq!(preview.files[1].hunks[0].lines[0].old_line, Some(10));
    assert_eq!(preview.files[1].note, None);
}

#[test]
fn intraline_ranges_are_unicode_safe_and_precise() {
    for (a, b, expected_a, expected_b) in [
        ("let café = '猫';", "let café = '犬';", "猫", "犬"),
        ("a🦀z", "a🐍z", "🦀", "🐍"),
        ("prefix old suffix", "prefix new suffix", "old", "new"),
        ("e\u{301}", "é", "e\u{301}", "é"),
    ] {
        let (ar, br) = changed_ranges(a, b);
        assert_eq!(&a[ar.unwrap()], expected_a);
        assert_eq!(&b[br.unwrap()], expected_b);
        let file = edit(a, b);
        for line in all_lines(&file) {
            if let Some(range) = &line.emphasis {
                assert!(line.text.is_char_boundary(range.start));
                assert!(line.text.is_char_boundary(range.end));
                assert!(range.start < range.end);
            }
        }
    }
    assert_eq!(changed_ranges("abc", "abXc"), (None, Some(2..3)));
    assert_eq!(changed_ranges("same", "same"), (None, None));
    assert_eq!(changed_ranges("", "🦀"), (None, Some(0..4)));
}

#[test]
fn intraline_pairing_never_crosses_context_or_hunks() {
    let file = from_patch(
        "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-old\n context\n+new\n@@ -10 +10 @@\n-abc\n+aXc\n",
    )
    .unwrap()
    .files
    .remove(0);
    assert!(file.hunks[0].lines.iter().all(|l| l.emphasis.is_none()));
    assert_eq!(file.hunks[1].lines[0].emphasis, Some(1..2));
    assert_eq!(file.hunks[1].lines[1].emphasis, Some(1..2));
}

#[test]
fn plain_text_keeps_identity_notes_hunks_markers_and_blank_lines() {
    let file = from_patch("*** Update File: old\n*** Move to: new\n@@ first\n-\n+hello\n\\ No newline at end of file\n@@ second\n-old\n+new\n*** End Patch").unwrap().files.remove(0);
    let text = file.plain_text();
    assert!(text.starts_with("Rename: old -> new\n"));
    assert!(text.contains("snippet-relative"));
    assert!(text.contains("\n-\n+hello\n\\ No newline at end of file\n"));
    assert!(text.contains("@@ second"));
}

#[test]
fn crlf_patch_protocol_is_supported() {
    let preview = from_patch("--- a/x\r\n+++ b/x\r\n@@ -1 +1 @@\r\n-old\r\n+new\r\n").unwrap();
    assert_eq!(preview.counts(), (1, 1));
    assert_eq!(preview.files[0].hunks[0].lines[0].text, "old");
}

#[test]
fn unprefixed_blank_context_is_preserved() {
    for patch in [
        "*** Update File: x\n@@\n\n-old\n+new\n",
        "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n\n-old\n+new\n",
    ] {
        let preview = from_patch(patch).unwrap();
        let lines = &preview.files[0].hunks[0].lines;
        assert_eq!(lines[0].kind, LineKind::Context);
        assert_eq!(lines[0].text, "");
        assert_eq!(lines[1].old_line, Some(2));
        assert_eq!(lines[2].new_line, Some(2));
        assert_eq!(preview.counts(), (1, 1));
    }
}

#[test]
fn large_sparse_edits_keep_common_interior_and_small_previews() {
    let old = (0..30_000)
        .map(|n| format!("unique line {n}\n"))
        .collect::<String>();
    let new = old
        .replace("unique line 100\n", "changed 100\n")
        .replace("unique line 29000\n", "changed 29000\n");
    let file = edit(&old, &new);
    assert_eq!(file.counts(), (2, 2));
    assert_eq!(file.hunks.len(), 2);
    assert!(all_lines(&file).len() < 20);
}

#[test]
fn large_anchorless_replacement_has_bounded_fallback() {
    let file = edit(&"old\n".repeat(20_000), &"new\n".repeat(20_000));
    assert_eq!(file.counts(), (20_000, 20_000));
    assert_eq!(file.hunks.len(), 1);
}

#[test]
fn large_repeated_interior_is_context_not_a_whole_file_replacement() {
    let middle = "repeated\n".repeat(25_000);
    let old = format!("old start\n{middle}old end\n");
    let new = format!("new start\n{middle}new end\n");
    let file = edit(&old, &new);
    assert_eq!(file.counts(), (2, 2));
    assert_eq!(file.hunks.len(), 2);
    assert!(all_lines(&file).len() < 20);
}

#[test]
fn bounded_fallback_preserves_input_order_on_repeated_shifted_lines() {
    let old = (0..1200).map(|i| i % 7).collect::<Vec<_>>();
    let mut new = old.clone();
    new.splice(100..103, [999, 998, 997, 996]);
    new.splice(800..809, [990]);
    let ops = diff_line_ids(&old, &new);
    let (mut a, mut b) = (0, 0);
    for op in ops {
        match op {
            Op::Equal(i, j) => {
                assert_eq!((i, j), (a, b));
                assert_eq!(old[i], new[j]);
                a += 1;
                b += 1;
            }
            Op::Delete(i) => {
                assert_eq!(i, a);
                a += 1;
            }
            Op::Insert(j) => {
                assert_eq!(j, b);
                b += 1;
            }
        }
    }
    assert_eq!((a, b), (old.len(), new.len()));
}

#[test]
fn ordinary_unified_paths_named_a_or_b_are_not_git_prefixes() {
    for path in ["a/foo", "b/foo", "normal/path"] {
        let preview = from_patch(&format!(
            "--- {path}\n+++ {path}\n@@ -1 +1 @@\n-old\n+new\n"
        ))
        .unwrap();
        assert_eq!(preview.files[0].path, path);
        assert_eq!(preview.files[0].kind, "Edit");
        assert_eq!(preview.files[0].previous_path, None);
    }
}

fn sequences() -> Vec<String> {
    let mut sequences = vec![String::new()];
    for len in 1..=5 {
        for bits in 0..(1 << len) {
            let s = (0..len)
                .map(|bit| if bits & (1 << bit) == 0 { "a\n" } else { "b\n" })
                .collect::<String>();
            sequences.push(s.clone());
            sequences.push(s.trim_end_matches('\n').to_owned());
        }
    }
    sequences
}

#[test]
fn exhaustive_small_diffs_reconstruct_both_inputs_and_are_minimal() {
    let seqs = sequences();
    for a in &seqs {
        for b in &seqs {
            let old = source_lines(a);
            let new = source_lines(b);
            let ops = diff_ops(&old, &new);
            let (mut ai, mut bi) = (0, 0);
            let mut equal_count = 0;
            for op in ops {
                match op {
                    Op::Equal(i, j) => {
                        assert_eq!((i, j), (ai, bi));
                        assert_eq!(old[i], new[j]);
                        ai += 1;
                        bi += 1;
                        equal_count += 1;
                    }
                    Op::Delete(i) => {
                        assert_eq!(i, ai);
                        ai += 1;
                    }
                    Op::Insert(j) => {
                        assert_eq!(j, bi);
                        bi += 1;
                    }
                }
            }
            assert_eq!((ai, bi), (old.len(), new.len()));
            // Independent brute-force common-subsequence oracle.
            let longest = (0usize..1 << old.len())
                .filter_map(|mask| {
                    let subseq = old
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| mask & (1 << i) != 0)
                        .map(|(_, line)| line)
                        .collect::<Vec<_>>();
                    let mut j = 0;
                    for line in &new {
                        if subseq.get(j) == Some(&line) {
                            j += 1;
                        }
                    }
                    (j == subseq.len()).then_some(j)
                })
                .max()
                .unwrap();
            assert_eq!(equal_count, longest, "{a:?} -> {b:?}");
        }
    }
}

#[test]
fn arbitrary_malformed_text_never_panics() {
    let alphabet = [
        "@@",
        "--- ",
        "+++ ",
        "*** Add File: ",
        "*** Update File: x",
        "diff --git a/x b/x",
        "@@ -1 +1 @@",
        "+",
        "-",
        " ",
        "\\ No newline at end of file",
        "🐍",
        "",
        "\0",
    ];
    let mut state = 13u64;
    for _ in 0..1500 {
        let mut input = String::new();
        for _ in 0..20 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            input.push_str(alphabet[(state as usize) % alphabet.len()]);
            input.push('\n');
        }
        if let Some(preview) = from_patch(&input) {
            let _ = preview.counts();
            for file in &preview.files {
                let _ = file.plain_text();
                for line in all_lines(file) {
                    if let Some(range) = &line.emphasis {
                        assert!(line.text.get(range.clone()).is_some());
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_hunk_counts_never_promote_body_lines_to_file_headers() {
    for body in [
        "@@ -0,0 +1,2 @@\n--- a/fake\n+++ b/fake\n",
        "@@ -1,2 +0,0 @@\n+++ b/not-a-header\n--- a/fake\n+++ b/fake\n",
        "@@ -1,x +1,2 @@\n--- a/fake\n+++ b/fake\n",
        "@@ -1,2 +1,2\n--- a/fake\n+++ b/fake\n",
        "@@ -1,2 +1,2 @@\n malformed context\n?invalid\n--- a/fake\n+++ b/fake\n",
        "@@ -1 +1 @@\n-old\n+new\n+surplus\n--- a/fake\n+++ b/fake\n",
        "@@ -1 +1 @@\n-old\n+new\n--- unpaired body\n?invalid\n--- a/fake\n+++ b/fake\n",
    ] {
        let patch = format!("--- a/real\n+++ b/real\n{body}");
        let preview = from_tool("patch", &patch).unwrap();
        assert_eq!(preview.files.len(), 1, "{body}");
        assert_eq!(preview.files[0].path, "real", "{body}");
        assert!(preview.files[0].note.is_some());
    }
}

#[test]
fn explicit_git_boundary_recovers_after_malformed_hunk() {
    let patch = "diff --git a/real b/real\n--- a/real\n+++ b/real\n@@ -0,0 +1,2 @@\n--- a/fake\n+++ b/fake\ndiff --git a/next b/next\n--- a/next\n+++ b/next\n@@ -11 +22 @@\n-a\n+b\n";
    let preview = from_tool("patch", patch).unwrap();
    assert_eq!(
        preview
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["real", "next"]
    );
    assert_eq!(preview.files[1].hunks[0].lines[0].old_line, Some(11));
    assert_eq!(preview.files[1].hunks[0].lines[1].new_line, Some(22));
    assert_eq!(preview.files[1].note, None);
}

#[test]
fn every_raw_patch_prefix_avoids_partial_or_content_filenames() {
    let patches = [
        (
            "functions.apply_patch",
            "*** Begin Patch\n*** Update File: 旧🦀.rs\n*** Move to: 新🦀.rs\n@@\n--- a/fake\n+++ b/fake\n*** Add File: added.txt\n+new\n*** Delete File: deleted.txt\n*** End Patch",
        ),
        (
            "patch",
            "diff --git a/旧🦀.rs b/新🦀.rs\nrename from 旧🦀.rs\nrename to 新🦀.rs\n--- a/旧🦀.rs\n+++ b/新🦀.rs\n@@ -1 +1 @@\n--- a/fake\n+++ b/fake\n--- /dev/null\n+++ b/added.txt\n@@ -0,0 +1 @@\n+new\n--- a/deleted.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n",
        ),
    ];
    let names = ["旧🦀.rs", "新🦀.rs", "added.txt", "deleted.txt"];
    for (name, patch) in patches {
        for end in patch
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(patch.len()))
        {
            if let Some(preview) = from_tool(name, &patch[..end]) {
                for file in preview.files {
                    assert!(
                        names.contains(&file.path.as_str()),
                        "{name} prefix {end}: {file:?}"
                    );
                    if let Some(previous) = file.previous_path {
                        assert!(
                            names.contains(&previous.as_str()),
                            "{name} prefix {end}: {previous}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn raw_headers_wait_for_newline_but_complete_payloads_do_not() {
    for input in [
        "*** Begin Patch\n*** Add File: par",
        "*** Begin Patch\n*** Update File: par",
        "*** Begin Patch\n*** Delete File: par",
        "diff --git a/partial b/par",
        "--- a/partial\n+++ b/par",
    ] {
        assert!(from_tool("apply_patch", input).is_none(), "{input}");
    }
    let patch = "*** Delete File: complete-name";
    assert_eq!(from_patch(patch).unwrap().files[0].path, "complete-name");
    assert_eq!(
        from_tool("patch", &json!({"patch_text":patch}).to_string())
            .unwrap()
            .files[0]
            .path,
        "complete-name"
    );
    let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n--- old\n+++ new";
    let preview = from_tool("patch", patch).unwrap();
    assert_eq!(preview.files[0].path, "x");
    assert_eq!(preview.counts(), (1, 1));
    assert_eq!(preview.files[0].hunks[0].lines[1].text, "++ new");
}

#[test]
fn incomplete_json_never_becomes_freeform_patch_text() {
    let input = json!({"patch_text":"*** Begin Patch\n*** Add File: real🦀\n+x\n*** End Patch"})
        .to_string();
    for (end, _) in input.char_indices() {
        assert!(from_tool("apply_patch", &input[..end]).is_none());
    }
    assert_eq!(
        from_tool("apply_patch", &input).unwrap().files[0].path,
        "real🦀"
    );
    assert!(
        from_tool(
            "apply_patch",
            "{\"patch_text\": \"\n*** Begin Patch\n*** Add File: fake\n+x\n"
        )
        .is_none()
    );
}

#[test]
fn metadata_only_git_paths_with_embedded_separator_are_not_split_early() {
    let preview = from_tool(
        "patch",
        "diff --git a/dir b/🦀 b/dir b/🦀\nold mode 100644\nnew mode 100755\n",
    )
    .unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "dir b/🦀");
    assert_eq!(preview.files[0].kind, "Edit");
    assert_eq!(preview.files[0].previous_path, None);
    for header in [
        "a/a space b/a space",
        "\"a/a space\" b/a space",
        "a/a space \"b/a space\"",
        "\"a/a space\" \"b/a space\"",
    ] {
        assert_eq!(
            git_paths(header),
            Some(("a space".to_owned(), "a space".to_owned()))
        );
    }
}

#[test]
fn null_to_null_headers_and_orphan_hunks_have_no_file() {
    for patch in [
        "@@ -1 +1 @@\n-a\n+b",
        "--- /dev/null\n+++ /dev/null\n",
        "--- \n+++ \n",
        "+++ b/orphan\n+x",
    ] {
        assert!(from_patch(patch).is_none(), "{patch}");
    }
}

#[test]
fn generated_multifile_hunks_never_change_content_ownership() {
    let content = [
        "",
        "++ b/fake",
        "-- a/fake",
        "🦀 日本語",
        "*** Update File: fake",
        "@@ -1 +1 @@",
    ];
    let mut patch = String::new();
    for (i, old) in content.iter().enumerate() {
        for (j, new) in content.iter().enumerate() {
            patch.push_str(&format!("--- a/file-{i}-{j}\n+++ b/file-{i}-{j}\n@@ -20,3 +30,3 @@ 🦀\n first\n-{old}\n+{new}\n last\n"));
        }
    }
    let preview = from_tool("patch", &patch).unwrap();
    assert_eq!(preview.files.len(), 36);
    for (i, file) in preview.files.iter().enumerate() {
        assert_eq!(file.path, format!("file-{}-{}", i / 6, i % 6));
        assert_eq!(file.counts(), (1, 1));
        assert_eq!(file.hunks[0].lines[1].text, content[i / 6]);
        assert_eq!(file.hunks[0].lines[2].text, content[i % 6]);
        assert_eq!(file.hunks[0].lines[1].old_line, Some(21));
        assert_eq!(file.hunks[0].lines[2].new_line, Some(31));
    }
}

fn replacement_block(old: &[&str], new: &[&str]) -> (Vec<DiffLine>, Vec<usize>, Vec<usize>) {
    let lines = old
        .iter()
        .map(|text| line(LineKind::Removed, text, None, None))
        .chain(
            new.iter()
                .map(|text| line(LineKind::Added, text, None, None)),
        )
        .collect();
    (
        lines,
        (0..old.len()).collect(),
        (old.len()..old.len() + new.len()).collect(),
    )
}

#[test]
fn review_fixture_pairs_title_replacement_after_inserted_comment() {
    let preview = from_patch(include_str!("../../../../assets/previews/change-review.diff")).unwrap();
    let lines = &preview.files[0].hunks[0].lines;
    let removed = lines
        .iter()
        .position(|line| line.text.trim() == "title.to_string()")
        .unwrap();
    let added = lines
        .iter()
        .position(|line| line.text.trim() == "title.chars().take(80).collect()")
        .unwrap();
    let comment = added - 1;
    assert_eq!(
        replacement_pairs(lines, &[removed], &[comment, added]),
        [(removed, added)]
    );
    assert!(lines[comment].emphasis.is_none());
    assert_eq!(
        &lines[removed].text[lines[removed].emphasis.clone().unwrap()],
        "to_string"
    );
    assert_eq!(
        &lines[added].text[lines[added].emphasis.clone().unwrap()],
        "chars().take(80).collect"
    );
}

#[test]
fn replacement_pairs_ignore_indentation_when_skipping_new_comments() {
    let (mut lines, removed, added) = replacement_block(
        &["                                title.to_string()"],
        &[
            "                                // explain the title",
            "title.chars().collect()",
        ],
    );
    assert_eq!(replacement_pairs(&lines, &removed, &added), [(0, 2)]);
    emphasize(&mut lines);
    assert!(lines[1].emphasis.is_none());
    assert!(lines[0].emphasis.is_some());
    assert!(lines[2].emphasis.is_some());
}

#[test]
fn replacement_pairs_skip_removed_comments_symmetrically() {
    let (mut lines, removed, added) = replacement_block(
        &["// obsolete description", "    title.to_string()"],
        &["\ttitle.chars().collect()"],
    );
    assert_eq!(replacement_pairs(&lines, &removed, &added), [(1, 2)]);
    emphasize(&mut lines);
    assert!(lines[0].emphasis.is_none());
    assert!(lines[1].emphasis.is_some());
    assert!(lines[2].emphasis.is_some());
}

#[test]
fn replacement_pairs_and_emphasis_are_unicode_safe() {
    let (mut lines, removed, added) = replacement_block(
        &["\u{3000}日本語🦀 = 猫;"],
        &["\u{3000}// 日本語の説明", "\t日本語🦀 = 犬;"],
    );
    assert_eq!(replacement_pairs(&lines, &removed, &added), [(0, 2)]);
    emphasize(&mut lines);
    assert!(lines[1].emphasis.is_none());
    for line in &lines {
        if let Some(range) = &line.emphasis {
            assert!(line.text.get(range.clone()).is_some());
        }
    }
    // Different indentation is genuinely part of the intraline change,
    // even though it is deliberately excluded from replacement scoring.
    assert!(lines[0].text[lines[0].emphasis.clone().unwrap()].contains('猫'));
    assert!(lines[2].text[lines[2].emphasis.clone().unwrap()].contains('犬'));
    assert!(replacement_similarity("日本語🦀.古い()", "日本語🦀.新しい()") > 0);
}

#[test]
fn equal_sized_replacements_keep_stable_positional_pairs() {
    let (lines, removed, added) = replacement_block(&["alpha()", "beta()"], &["beta()", "alpha()"]);
    assert_eq!(
        replacement_pairs(&lines, &removed, &added),
        [(0, 2), (1, 3)]
    );
    let (lines, removed, added) = replacement_block(&["alpha()"], &["wholly different"]);
    assert_eq!(replacement_pairs(&lines, &removed, &added), [(0, 1)]);
}

#[test]
fn replacement_pairs_preserve_both_orders_and_nearest_ties() {
    let (lines, removed, added) = replacement_block(
        &["alpha()", "beta()"],
        &["// first", "alpha(1)", "// second", "beta(2)"],
    );
    assert_eq!(
        replacement_pairs(&lines, &removed, &added),
        [(0, 3), (1, 5)]
    );
    let (lines, removed, added) = replacement_block(&["x", "y"], &["a", "b", "c"]);
    assert_eq!(
        replacement_pairs(&lines, &removed, &added),
        [(0, 2), (1, 3)]
    );
    for old_len in 0..24 {
        for new_len in 0..24 {
            let old = (0..old_len)
                .map(|i| ["alpha()", "beta()", "gamma()"][i % 3])
                .collect::<Vec<_>>();
            let new = (0..new_len)
                .map(|i| ["// comment", "gamma()", "beta()", "alpha()"][i % 4])
                .collect::<Vec<_>>();
            let (lines, removed, added) = replacement_block(&old, &new);
            let pairs = replacement_pairs(&lines, &removed, &added);
            assert_eq!(pairs.len(), old_len.min(new_len));
            assert!(
                pairs
                    .windows(2)
                    .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1)
            );
            assert!(
                pairs
                    .iter()
                    .all(|(a, b)| removed.contains(a) && added.contains(b))
            );
        }
    }
}

#[test]
fn replacement_pair_lookahead_and_long_line_scoring_are_capped() {
    let mut additions = vec!["// unrelated comment"; 32];
    additions.push("target(1)");
    let (lines, removed, added) = replacement_block(&["target()"], &additions);
    // A candidate beyond the fixed window is intentionally not scanned.
    assert_eq!(replacement_pairs(&lines, &removed, &added), [(0, 1)]);
    let long = "🦀".repeat(100_000);
    assert_eq!(replacement_similarity(&long, &long), 128 * 3);
    let (lines, removed, added) = replacement_block(&vec!["old"; 20_000], &vec!["new"; 40_000]);
    let pairs = replacement_pairs(&lines, &removed, &added);
    assert_eq!(pairs.len(), 20_000);
    assert_eq!(pairs[0], (0, 20_000));
    assert_eq!(pairs[19_999], (19_999, 39_999));
}

#[test]
fn replacement_pairs_ignore_nonmatching_kinds_and_invalid_indices() {
    let (lines, _, _) = replacement_block(&["old"], &["new", "// extra"]);
    assert_eq!(
        replacement_pairs(&lines, &[0, usize::MAX], &[1, 2]),
        [(0, 1)]
    );
    assert!(replacement_pairs(&lines, &[1], &[0]).is_empty());
    assert!(replacement_pairs(&lines, &[], &[1, 2]).is_empty());
}
