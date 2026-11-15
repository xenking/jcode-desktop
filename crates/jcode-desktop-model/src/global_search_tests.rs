//! State-machine tests for the global search screen.

use crate::global_search::*;

fn hit(session_id: &str, title: &str, role: Role, snippet: &str) -> SearchHit {
    SearchHit {
        session_id: session_id.into(),
        title: title.into(),
        role,
        snippet: snippet.into(),
        highlights: Vec::new(),
        timestamp_ms: 1_767_323_045_000,
        message_id: Some("m1".into()),
    }
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
fn debounce_is_150ms_and_blank_query_sends_nothing() {
    let mut screen = Screen::new();
    assert_eq!(screen.debounce(), std::time::Duration::from_millis(150));
    assert_eq!(screen.status(), Status::Idle);
    assert_eq!(screen.set_query("   "), Effect::Cleared);
    assert_eq!(screen.status(), Status::Idle);
    assert!(screen.rows().is_empty());
    let Effect::Debounce(generation) = screen.set_query("walrus") else {
        panic!("typing must schedule a debounced query");
    };
    assert_eq!(screen.status(), Status::Waiting);
    assert!(screen.mark_sent(generation));
    assert_eq!(screen.status(), Status::Searching);
}

#[test]
fn identical_query_text_does_not_restart_the_debounce() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("abc") else {
        panic!("first keystroke schedules");
    };
    assert_eq!(screen.set_query("abc"), Effect::Cleared);
    // The generation is unchanged, so the in-flight debounce stays valid.
    assert_eq!(screen.pending_query(), ("abc", generation));
}

#[test]
fn stale_replies_are_dropped() {
    let mut screen = Screen::new();
    let Effect::Debounce(first) = screen.set_query("w") else {
        panic!()
    };
    let Effect::Debounce(second) = screen.set_query("wa") else {
        panic!()
    };
    assert!(second > first);
    assert!(!screen.mark_sent(first), "an older send is refused");
    assert!(screen.mark_sent(second));
    // The answer to the abandoned query arrives late.
    assert!(!screen.apply_results(
        first,
        results(vec![], vec![hit("s1", "t", Role::User, "walrus")]),
    ));
    assert_eq!(screen.status(), Status::Searching);
    assert!(screen.apply_results(second, results(vec![], vec![])));
    assert_eq!(screen.status(), Status::Ready);
}

#[test]
fn titles_come_first_and_messages_are_grouped_by_session() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("walrus") else {
        panic!()
    };
    screen.mark_sent(generation);
    screen.apply_results(
        generation,
        results(
            vec![hit("s_title", "Walrus plans", Role::Title, "Walrus plans")],
            vec![
                hit("s_a", "Alpha", Role::User, "walrus one"),
                hit("s_b", "Beta", Role::Assistant, "walrus two"),
                hit("s_a", "Alpha", Role::Assistant, "walrus three"),
            ],
        ),
    );
    let rows = screen.rows();
    assert_eq!(
        rows,
        &[
            Row::Section("Titles"),
            Row::Hit {
                hit: 0,
                role: Role::Title
            },
            Row::Section("Messages"),
            Row::Group {
                session_id: "s_a".into(),
                title: "Alpha".into()
            },
            Row::Hit {
                hit: 0,
                role: Role::User
            },
            Row::Hit {
                hit: 2,
                role: Role::Assistant
            },
            Row::Group {
                session_id: "s_b".into(),
                title: "Beta".into()
            },
            Row::Hit {
                hit: 1,
                role: Role::Assistant
            },
        ]
    );
}

#[test]
fn keyboard_navigation_skips_headers_and_clamps() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("x") else {
        panic!()
    };
    screen.mark_sent(generation);
    screen.apply_results(
        generation,
        results(
            vec![hit("s_title", "T", Role::Title, "x")],
            vec![
                hit("s_a", "Alpha", Role::User, "x"),
                hit("s_a", "Alpha", Role::Assistant, "x"),
            ],
        ),
    );
    // The first selectable row is selected on arrival.
    assert_eq!(screen.selected(), Some(1));

    screen.move_selection(1);
    assert_eq!(
        screen.selected(),
        Some(4),
        "jumps over the Messages section header and the group header"
    );
    screen.move_selection(1);
    assert_eq!(screen.selected(), Some(5));
    screen.move_selection(1);
    assert_eq!(screen.selected(), Some(5), "clamped at the last hit");
    screen.move_selection(-2);
    assert_eq!(screen.selected(), Some(1));
    screen.move_selection(-1);
    assert_eq!(screen.selected(), Some(1), "clamped at the first hit");

    // A click on a header is ignored.
    screen.select_row(3);
    assert_eq!(screen.selected(), Some(1));
    screen.select_row(4);
    assert_eq!(screen.selected(), Some(4));
}

#[test]
fn enter_on_a_title_opens_the_session_and_on_a_message_reveals_its_text() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("walrus") else {
        panic!()
    };
    screen.mark_sent(generation);
    let mut message = hit(
        "s_a",
        "Alpha",
        Role::User,
        "…the walrus was Paul…",
    );
    message.highlights = vec![Highlight { start: 8, end: 14 }];
    screen.apply_results(
        generation,
        results(
            vec![hit("s_title", "Walrus plans", Role::Title, "Walrus plans")],
            vec![message],
        ),
    );

    let title_target = screen.open_target().expect("a title row is selected");
    assert_eq!(title_target.session_id, "s_title");
    assert_eq!(title_target.reveal, None);

    screen.move_selection(1);
    let message_target = screen.open_target().expect("a message row is selected");
    assert_eq!(message_target.session_id, "s_a");
    assert_eq!(message_target.reveal.as_deref(), Some("the walrus was Paul"));
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
    message.highlights = vec![Highlight {
        start: 34,
        end: 40,
    }];
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

#[test]
fn empty_results_and_failures_have_honest_status_lines() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("nothing") else {
        panic!()
    };
    screen.mark_sent(generation);
    screen.apply_results(generation, results(vec![], vec![]));
    assert_eq!(screen.status_line(), "No matches in 12 sessions");
    assert_eq!(screen.selected(), None);
    assert_eq!(screen.open_target(), None);

    let Effect::Debounce(generation) = screen.set_query("boom") else {
        panic!()
    };
    screen.mark_sent(generation);
    assert!(screen.apply_error(generation, "daemon unavailable".into()));
    assert_eq!(screen.status(), Status::Failed);
    assert_eq!(screen.status_line(), "daemon unavailable");
    assert!(screen.rows().is_empty());
}

#[test]
fn clearing_the_query_forgets_the_previous_results() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("walrus") else {
        panic!()
    };
    screen.mark_sent(generation);
    screen.apply_results(
        generation,
        results(vec![], vec![hit("s_a", "Alpha", Role::User, "walrus")]),
    );
    assert_eq!(screen.set_query(""), Effect::Cleared);
    assert!(screen.results().is_none());
    assert!(screen.rows().is_empty());
    assert_eq!(screen.status(), Status::Idle);
    // A late reply for the abandoned query must not repopulate the screen.
    assert!(!screen.apply_results(
        generation,
        results(vec![], vec![hit("s_a", "Alpha", Role::User, "walrus")]),
    ));
}

#[test]
fn truncation_and_singular_counts_read_naturally() {
    let mut screen = Screen::new();
    let Effect::Debounce(generation) = screen.set_query("x") else {
        panic!()
    };
    screen.mark_sent(generation);
    let mut payload = results(
        vec![hit("s1", "T", Role::Title, "x")],
        vec![hit("s2", "t", Role::User, "x")],
    );
    payload.truncated = true;
    screen.apply_results(generation, payload);
    assert_eq!(screen.status_line(), "1+ title · 1+ message");
}
