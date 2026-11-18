//! Session search is the session picker: Cmd+Shift+F and the sidebar
//! magnifier open the same screen as `/resume`.
//!
//! The row/selection/debounce logic is tested in `jcode-desktop-model`; these
//! tests prove the wiring a model test cannot: that the picker replaces the
//! workspace instead of floating over it, that the sidebar magnifier opens it,
//! that its field keeps the keyboard, that message hits appear under their
//! session, and that opening one marks the query in the opened session.

use super::*;

#[gpui::test]
fn search_screen_takes_the_whole_window_and_escape_restores_the_workspace(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |p, cx| {
        p.input.update(cx, |input, cx| {
            input.set_content("valuable draft".into(), cx)
        })
    });
    vcx.run_until_parked();
    let canvas = vcx
        .debug_bounds("workspace-canvas")
        .expect("canvas rendered");
    assert!(
        vcx.debug_bounds("resume-panel").is_none(),
        "the screen is closed until asked for"
    );

    workspace.update_in(vcx, |w, window, cx| {
        w.open_session_search(&GlobalSearch, window, cx);
        assert!(w.resume_open());
    });
    vcx.run_until_parked();

    let screen = vcx
        .debug_bounds("resume-panel")
        .expect("the search screen rendered");
    assert!(
        vcx.debug_bounds("workspace-canvas").is_none(),
        "a full-height screen replaces the canvas rather than floating over it"
    );
    assert!(
        screen.size.height >= canvas.size.height * 0.95
            && screen.size.width >= canvas.size.width * 0.95,
        "screen {screen:?} must fill the {canvas:?} the canvas had"
    );
    assert!(
        vcx.debug_bounds("resume-search").is_some(),
        "the single query field is focused chrome, not a popover trigger"
    );
    assert!(
        vcx.debug_bounds("resume-results").is_some(),
        "the results list is present before the first query"
    );
    vcx.simulate_input("query");
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "valuable draft"
    );

    // Escape is the documented way out, and it is a real key path rather than
    // a close button only.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        !workspace.read_with(vcx, |w, _| w.resume_open()),
        "escape closes the screen"
    );
    assert!(
        vcx.debug_bounds("workspace-canvas").is_some(),
        "the workspace comes back"
    );
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "valuable draft"
    );
}

#[gpui::test]
fn sidebar_search_remains_clickable_during_ask(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w.show_sidebar = true;
        w
    });
    vcx.run_until_parked();
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
    for _ in 0..120 {
        if vcx.debug_bounds("composer-dock").is_some() {
            break;
        }
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"chat","request_id":"r","tool_call_id":"t","questions":[{"question":"Pick one","options":[{"label":"A"},{"label":"B"}]}]}"#).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-card").is_some());
    let button = vcx
        .debug_bounds("sidebar-search-icon")
        .or_else(|| vcx.debug_bounds("sidebar-search"))
        .unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(workspace.read_with(vcx, |w, _| w.resume_open()));
}

#[gpui::test]
fn sidebar_magnifier_opens_the_search_screen(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w.show_sidebar = true;
        w
    });
    vcx.run_until_parked();
    // Compact and wide sidebars expose the same action in different chrome, so
    // accept either shape and click it.
    let button = vcx
        .debug_bounds("sidebar-search-icon")
        .or_else(|| vcx.debug_bounds("sidebar-search"))
        .expect("the sidebar renders a magnifier");
    assert!(!workspace.read_with(vcx, |w, _| w.resume_open()));
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        workspace.read_with(vcx, |w, _| w.resume_open()),
        "clicking the magnifier opens global search"
    );
}

/// The screen is useless if the query field never receives keystrokes. This
/// goes through the real dispatch path (keybindings, capture/bubble, IME)
/// instead of setting the text, because the bug this pins was invisible to a
/// test that assigned `content` directly.
#[gpui::test]
fn typing_into_the_search_screen_reaches_the_query_field(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    workspace.update_in(vcx, |w, window, cx| {
        w.open_session_search(&GlobalSearch, window, cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    let typed = workspace.read_with(vcx, |w, cx| {
        w.resume
            .as_ref()
            .map(|state| state.search.read(cx).content.to_string())
            .unwrap_or_default()
    });
    assert_eq!(
        typed, "snap",
        "typed characters must land in the query field"
    );
}

fn search_query(workspace: &gpui::Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> String {
    workspace.read_with(cx, |w, cx| {
        w.resume
            .as_ref()
            .map(|state| state.search.read(cx).content.to_string())
            .unwrap_or_default()
    })
}

fn open_search(workspace: &gpui::Entity<Workspace>, vcx: &mut gpui::VisualTestContext) {
    workspace.update_in(vcx, |w, window, cx| {
        w.open_session_search(&GlobalSearch, window, cx);
    });
    vcx.run_until_parked();
}

/// Clicking the screen's own chrome (the padding around the field, the counts,
/// the empty results area) used to hand focus back to the workspace handle the
/// screen root tracks: the caret vanished and every later keystroke was lost.
/// The single field must keep the keyboard wherever the click landed.
#[gpui::test]
fn clicking_the_search_chrome_keeps_the_query_field_typing(cx: &mut gpui::TestAppContext) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    let list = vcx
        .debug_bounds("resume-results")
        .expect("the results area is on screen");
    // Empty results area: part of the screen, but not part of the field. This
    // click used to hand focus to the workspace handle the root tracks.
    vcx.simulate_click(
        gpui::point(list.center().x, list.bottom() - px(40.0)),
        gpui::Modifiers::default(),
    );
    vcx.run_until_parked();
    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    assert_eq!(
        search_query(&workspace, &mut vcx),
        "snap",
        "a click anywhere on the screen must leave the query field typing"
    );
}

/// `focus_active` runs after shortcuts and layout changes and targets a panel
/// that the full-window screen does not render. Moving focus there leaves a
/// stale caret on the field and swallows everything typed afterwards.
#[gpui::test]
fn focusing_the_active_panel_while_searching_keeps_the_query_field_typing(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    workspace.update_in(vcx, |w, window, cx| w.focus_active(window, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    assert_eq!(
        search_query(&workspace, &mut vcx),
        "snap",
        "the search field keeps the keyboard while the screen is open"
    );
    assert!(
        workspace.read_with(vcx, |w, _| w.resume_open()),
        "regaining focus must not close or replace the screen"
    );
}

/// The screen replaces the canvas, so it draws where the window's titlebar is.
/// On macOS that puts its heading under the traffic lights unless the screen
/// reserves the same inset the single-panel workspace does.
#[gpui::test]
fn the_search_heading_clears_the_titlebar_inset(cx: &mut gpui::TestAppContext) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    let screen = vcx
        .debug_bounds("resume-panel")
        .expect("the search screen rendered");
    let title = vcx
        .debug_bounds("resume-title")
        .expect("the heading rendered");
    let inset = content_top_inset(false, false);
    assert!(
        title.top() >= screen.top() + px(inset),
        "heading {title:?} must sit below the {inset}px titlebar inset inside {screen:?}"
    );
}

fn hit(
    session_id: &str,
    role: jcode_sdk::SessionSearchRole,
    snippet: &str,
) -> jcode_sdk::SessionSearchHit {
    let start = snippet.find("walrus").unwrap_or_default() as u32;
    jcode_sdk::SessionSearchHit {
        session_id: session_id.into(),
        title: "Zoo notes".into(),
        role,
        model: None,
        snippet: snippet.into(),
        highlights: vec![jcode_sdk::SessionSearchHighlight {
            start,
            end: start + 6,
        }],
        timestamp_ms: 1,
        message_id: None,
    }
}

/// Typing sends the query to the daemon after the debounce, the reply lists
/// message hits under their session, and Enter on a hit opens that session
/// with the query marked in its transcript.
#[gpui::test]
fn message_hits_list_under_their_session_and_open_marked(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.set_test_bridge(bridge);
        w.push_test_panel("chat", cx);
        w.sessions = vec![
            tests::session_info("s_zoo", Some("Zoo notes")),
            tests::session_info("s_other", Some("Other work")),
        ];
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, vcx);
    vcx.simulate_input("walrus");
    vcx.executor()
        .advance_clock(crate::global_search::DEBOUNCE + std::time::Duration::from_millis(10));
    vcx.run_until_parked();
    let generation = std::iter::from_fn(|| commands.try_recv().ok())
        .find_map(|command| match command {
            Command::SearchSessions { query, generation } if query == "walrus" => Some(generation),
            _ => None,
        })
        .expect("the debounced query reaches the daemon");

    workspace.update(vcx, |w, cx| {
        w.apply_session_search(
            "walrus".into(),
            generation,
            Ok(jcode_sdk::SessionSearchResults {
                query: "walrus".into(),
                titles: vec![],
                messages: vec![
                    hit(
                        "s_zoo",
                        jcode_sdk::SessionSearchRole::User,
                        "where is the walrus now",
                    ),
                    hit(
                        "s_zoo",
                        jcode_sdk::SessionSearchRole::Assistant,
                        "the walrus sleeps",
                    ),
                ],
                indexed_sessions: 2,
                truncated: false,
            }),
            3.0,
            cx,
        );
    });
    vcx.run_until_parked();
    // Only the matching session is listed, with both hits under it.
    assert!(vcx.debug_bounds("resume-row-0").is_some());
    assert!(
        vcx.debug_bounds("resume-row-1").is_none(),
        "the unrelated session is filtered out"
    );
    assert!(vcx.debug_bounds("resume-hit-0-0").is_some());
    assert!(vcx.debug_bounds("resume-hit-0-1").is_some());

    // Down from the session row selects its first hit; Enter opens it.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    let selected_hit =
        workspace.read_with(vcx, |w, _| w.resume.as_ref().and_then(|s| s.selected_hit));
    assert_eq!(selected_hit, Some(0));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!workspace.read_with(vcx, |w, _| w.resume_open()));
    let (session, terms) = workspace.read_with(vcx, |w, cx| {
        let panel = w.slots[w.active].panel.read(cx);
        (panel.session_id.clone(), panel.search_terms.clone())
    });
    assert_eq!(session, "s_zoo");
    assert_eq!(
        terms,
        vec!["walrus".to_string()],
        "the opened session marks the query"
    );
}
