//! Tests for the session picker's search model.

use crate::global_search::*;

fn hit(session_id: &str, title: &str, role: Role, snippet: &str) -> SearchHit {
    SearchHit {
        session_id: session_id.into(),
        title: title.into(),
        role,
        model: None,
        snippet: snippet.into(),
        highlights: Vec::new(),
        timestamp_ms: 1_767_323_045_000,
        message_id: Some("m1".into()),
    }
}

/// The hit row must be one line: the FTS snippet holds the message's own line
/// breaks, and rendering them turns one hit into a broken multi-row block.
#[test]
fn a_snippet_is_one_line_and_windowed_around_the_match() {
    let long_preamble = "x".repeat(300);
    let snippet = format!(
        "{long_preamble}\n\n  first line\n  the walrus\t\tindexes\n  trailing text that goes on and on and on and on and never stops at all because messages are long"
    );
    let match_start = snippet.find("walrus").unwrap() as u32;
    let (text, highlights) = window_snippet(
        &snippet,
        &[Highlight {
            start: match_start,
            end: match_start + 6,
        }],
    );
    assert!(!text.contains('\n'), "newlines collapsed: {text:?}");
    assert!(!text.contains("  "), "whitespace runs collapsed: {text:?}");
    assert!(
        text.starts_with('…'),
        "text before the window is elided: {text:?}"
    );
    assert!(
        text.chars().count() <= SNIPPET_CHARS + 2 && text.chars().count() >= 80,
        "one row's worth: {text:?}"
    );
    assert_eq!(highlights.len(), 1);
    let marked: String = text
        .chars()
        .skip(highlights[0].start as usize)
        .take((highlights[0].end - highlights[0].start) as usize)
        .collect();
    assert_eq!(
        marked, "walrus",
        "the highlight still marks the match: {text:?}"
    );
    let lead = highlights[0].start as usize - 1;
    assert!(
        lead <= SNIPPET_LEAD + 8,
        "about {SNIPPET_LEAD} characters of context before the match, got {lead}"
    );
}

#[test]
fn a_short_snippet_keeps_its_text_and_highlights_aligned() {
    let (text, highlights) = window_snippet(
        "before\n  the walrus  index after",
        &[Highlight { start: 13, end: 19 }],
    );
    assert_eq!(text, "before the walrus index after");
    let marked: String = text
        .chars()
        .skip(highlights[0].start as usize)
        .take((highlights[0].end - highlights[0].start) as usize)
        .collect();
    assert_eq!(marked, "walrus");
}

#[test]
fn an_assistant_badge_names_its_model() {
    let mut assistant = hit("s", "t", Role::Assistant, "text");
    assistant.model = Some("claude-opus-5-5".into());
    assert_eq!(assistant.badge(), "opus 5.5");
    assistant.model = Some("deepseek-v4.1-flash".into());
    assert_eq!(assistant.badge(), "ds-4.1f");
    assistant.model = None;
    assert_eq!(assistant.badge(), "jcode");
    assert_eq!(hit("s", "t", Role::User, "text").badge(), "You");
}

#[test]
fn a_hit_time_is_absolute_and_relative() {
    // 2026-11-15T10:40:00Z, three hours before "now".
    let timestamp = 1_763_193_600_000;
    let label = time_label(timestamp, timestamp + 3 * 3_600_000);
    assert!(label.ends_with("· 3h"), "{label}");
    assert!(label.contains("Nov"), "absolute date in the label: {label}");
    assert!(label.contains(':'), "hour and minute in the label: {label}");
    assert_eq!(time_label(0, timestamp), "");
}

fn results(titles: Vec<SearchHit>, messages: Vec<SearchHit>) -> Results {
    Results {
        query: "q".into(),
        titles,
        messages,
        indexed_sessions: 12,
        truncated: false,
    }
}

#[test]
fn reveal_needle_prefers_the_fragment_holding_the_match() {
    let mut message = hit(
        "s",
        "t",
        Role::Assistant,
        "first fragment about nothing …  the needle\n\n  is here  … trailing words",
    );
    // Offset inside the second fragment ("the needle is here").
    message.highlights = vec![Highlight { start: 34, end: 40 }];
    assert_eq!(
        reveal_needle(&message),
        "the needle is here",
        "whitespace is collapsed and the matching fragment wins"
    );

    // With no ellipses the whole snippet is contiguous.
    let mut whole = hit("s", "t", Role::User, "a plain snippet");
    whole.highlights = vec![Highlight { start: 0, end: 1 }];
    assert_eq!(reveal_needle(&whole), "a plain snippet");
}

fn searched(query: &str, payload: Results) -> Search {
    let mut search = Search::new();
    let Effect::Debounce(generation) = search.set_query(query) else {
        panic!("a real query schedules a search")
    };
    assert!(search.mark_sent(generation));
    assert!(search.apply_results(generation, payload, 12.0));
    search
}

#[test]
fn blank_and_single_letter_queries_send_nothing() {
    let mut search = Search::new();
    assert_eq!(DEBOUNCE, std::time::Duration::from_millis(150));
    assert_eq!(search.set_query("   "), Effect::Cleared);
    assert_eq!(search.set_query("a b"), Effect::Cleared);
    assert_eq!(search.status(), Status::Idle);
    assert!(search.status_line().is_none());
    let Effect::Debounce(generation) = search.set_query("walrus") else {
        panic!("typing must schedule a debounced query");
    };
    assert!(search.pending());
    assert_eq!(
        search.set_query("walrus"),
        Effect::Cleared,
        "unchanged text keeps the pending send"
    );
    assert_eq!(search.pending_query(), ("walrus", generation));
}

#[test]
fn stale_replies_are_dropped_and_clearing_forgets_results() {
    let mut search = Search::new();
    let Effect::Debounce(first) = search.set_query("wa") else {
        panic!()
    };
    let Effect::Debounce(second) = search.set_query("wal") else {
        panic!()
    };
    assert!(!search.mark_sent(first), "an older send is refused");
    assert!(search.mark_sent(second));
    let payload = || results(vec![], vec![hit("s1", "t", Role::User, "walrus")]);
    assert!(!search.apply_results(first, payload(), 1.0));
    assert!(search.pending());
    assert!(search.apply_results(second, payload(), 1.0));
    assert_eq!(search.status(), Status::Ready);

    assert_eq!(search.set_query(""), Effect::Cleared);
    assert!(search.results().is_none());
    assert!(
        !search.apply_results(second, payload(), 1.0),
        "a late reply must not repopulate a cleared picker"
    );
}

#[test]
fn rows_put_each_sessions_hits_under_it_capped() {
    let search = searched(
        "walrus",
        results(
            vec![hit("s_title", "Walrus plans", Role::Title, "Walrus plans")],
            vec![
                hit("s_a", "Alpha", Role::User, "walrus one"),
                hit("s_b", "Beta", Role::Assistant, "walrus two"),
                hit("s_a", "Alpha", Role::Assistant, "walrus three"),
                hit("s_a", "Alpha", Role::User, "walrus four"),
                hit("s_a", "Alpha", Role::User, "walrus five"),
            ],
        ),
    );
    assert_eq!(
        search.matched_sessions(),
        vec![
            ("s_title".into(), "Walrus plans".into()),
            ("s_a".into(), "Alpha".into()),
            ("s_b".into(), "Beta".into()),
        ],
        "title matches outrank message matches, each session once"
    );
    let rows = search.rows(&["s_a".into(), "s_title".into(), "s_b".into()]);
    let hit_row = |hit| PickRow::Hit {
        session_id: "s_a".into(),
        hit,
    };
    assert_eq!(
        rows,
        vec![
            PickRow::Session("s_a".into()),
            hit_row(0),
            hit_row(2),
            hit_row(3),
            PickRow::Session("s_title".into()),
            PickRow::Session("s_b".into()),
            PickRow::Hit {
                session_id: "s_b".into(),
                hit: 1
            },
        ]
    );
    assert_eq!(
        rows.iter().filter(|row| row.session_id() == "s_a").count(),
        1 + HITS_PER_SESSION
    );
}

#[test]
fn opening_a_row_reveals_the_matching_message() {
    let mut message = hit("s_a", "Alpha", Role::User, "…the walrus was Paul…");
    message.highlights = vec![Highlight { start: 5, end: 11 }];
    let search = searched(
        "walrus",
        results(
            vec![hit("s_t", "Walrus plans", Role::Title, "Walrus plans")],
            vec![message],
        ),
    );
    let needle = Some("the walrus was Paul".to_string());
    assert_eq!(
        search.reveal_for(&PickRow::Hit {
            session_id: "s_a".into(),
            hit: 0
        }),
        needle
    );
    assert_eq!(
        search.reveal_for(&PickRow::Session("s_a".into())),
        needle,
        "a session row opens at its best hit"
    );
    assert_eq!(
        search.reveal_for(&PickRow::Session("s_t".into())),
        None,
        "a title-only match opens normally"
    );
}

#[test]
fn keyboard_steps_clamp_and_recover_a_lost_selection() {
    let rows = vec![
        PickRow::Session("a".into()),
        PickRow::Hit {
            session_id: "a".into(),
            hit: 0,
        },
        PickRow::Session("b".into()),
    ];
    assert_eq!(step(&rows, None, 1), Some(rows[0].clone()));
    assert_eq!(step(&rows, Some(&rows[0]), 1), Some(rows[1].clone()));
    assert_eq!(step(&rows, Some(&rows[2]), 8), Some(rows[2].clone()));
    assert_eq!(step(&rows, Some(&rows[1]), -8), Some(rows[0].clone()));
    assert_eq!(step(&[], None, 1), None);
}

#[test]
fn status_line_reports_progress_counts_and_failures() {
    let mut payload = results(vec![], vec![hit("s", "t", Role::User, "x")]);
    payload.truncated = true;
    assert_eq!(
        searched("xy", payload).status_line().as_deref(),
        Some("1+ matching message · 12ms")
    );
    let mut search = Search::new();
    let Effect::Debounce(generation) = search.set_query("boom") else {
        panic!()
    };
    assert_eq!(
        search.status_line().as_deref(),
        Some("searching conversations…")
    );
    search.mark_sent(generation);
    assert!(search.apply_error(generation, "daemon unavailable".into()));
    assert_eq!(
        search.status_line().as_deref(),
        Some("conversation search unavailable: daemon unavailable")
    );
}

#[test]
fn query_terms_strip_syntax_fold_case_and_drop_noise() {
    assert_eq!(
        query_terms("  \"ЁЖИК\" Walrus a walrus) "),
        vec!["ежик", "walrus"]
    );
    assert!(query_terms("a ( b").is_empty());
}

#[test]
fn term_ranges_mark_every_case_insensitive_occurrence() {
    let text = "Walrus, walrus! Ёжик и ежик";
    let ranges = term_ranges(text, &query_terms("walrus ежик"));
    let marked: Vec<&str> = ranges.iter().map(|range| &text[range.clone()]).collect();
    assert_eq!(marked, vec!["Walrus", "walrus", "Ёжик", "ежик"]);
    assert!(term_ranges(text, &[]).is_empty());
    // Overlapping terms keep the longer match and never overlap ranges.
    let ranges = term_ranges("rollout", &query_terms("roll rollout"));
    assert_eq!(ranges, vec![0..7]);
}
