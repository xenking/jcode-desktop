//! The `+` menu: modes with their own icons, groups split by separators, a
//! popup that never changes size, and the MCP submenu.
use super::*;
use crate::input::mcp_servers::Server;

fn open_menu<'a>(
    cx: &'a mut gpui::TestAppContext,
    skills: &[(&str, &str)],
    mcp: Option<Vec<Server>>,
) -> (Entity<Panel>, &'a mut gpui::VisualTestContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.skills = skills
            .iter()
            .map(|(name, help)| ((*name).into(), (*help).into()))
            .collect();
        panel.mcp_servers = mcp;
        panel.sync_mode_menu(cx);
        cx.notify();
    });
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.open_picker_search(PickerKind::Context, cx)
    });
    (panel, vcx)
}

fn server(name: &str, summary: &str) -> Server {
    Server {
        name: name.into(),
        summary: summary.into(),
    }
}

fn values(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> Vec<String> {
    panel.read_with(vcx, |panel, cx| {
        let (_, search) = panel.picker_search.as_ref().expect("menu open");
        search.read(cx).command_suggestion_values()
    })
}

const FOUR: [(&str, &str); 4] = [
    ("plan", "Generate an implementation plan"),
    ("debug", "Pinpoint the root cause of an issue"),
    ("multitask", "Orchestrate multiple subagents in parallel"),
    ("ask", "Answer questions without making edits"),
];

#[gpui::test]
fn every_mode_row_has_its_own_icon_and_the_groups_have_no_titles(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open_menu(cx, &FOUR, Some(vec![server("alpha", "local server")]));
    vcx.run_until_parked();
    let rows = values(&panel, vcx);
    for (mode, icon) in [
        ("plan", "plan"),
        ("debug", "bug"),
        ("multitask", "multitask"),
        ("ask", "help"),
    ] {
        let index = rows
            .iter()
            .position(|row| *row == format!("/mode {mode}"))
            .expect("listed");
        let selector = format!("mode-picker-icon-{icon}-{index}");
        assert!(
            vcx.debug_bounds(Box::leak(selector.clone().into_boxed_str()))
                .is_some(),
            "{selector} drawn"
        );
    }
    assert!(
        (0..rows.len()).all(|index| vcx
            .debug_bounds(Box::leak(
                format!("model-picker-group-{index}").into_boxed_str()
            ))
            .is_none()),
        "no section title rows, only separators"
    );
    let tools = rows
        .iter()
        .position(|row| row == "Attach…")
        .expect("tools listed");
    let skills = rows
        .iter()
        .position(|row| row == "/mode plan")
        .expect("skills listed");
    assert_eq!(
        tools, 0,
        "no modes are active, so the tools lead and need no separator"
    );
    assert!(vcx.debug_bounds("picker-separator-0").is_none());
    assert!(
        vcx.debug_bounds(Box::leak(
            format!("picker-separator-{skills}").into_boxed_str()
        ))
        .is_some(),
        "a separator before the skills"
    );
}

#[gpui::test]
fn the_menu_is_exactly_as_tall_as_its_rows_from_the_first_frame(cx: &mut gpui::TestAppContext) {
    // A list that fits has no slack below its last row: no row height is an
    // estimate. A list that scrolls fills its cap. Both hold on every frame.
    for rows in [1, 3, 6, 40] {
        let skills: Vec<_> = (0..rows)
            .map(|index| {
                (
                    format!("skill-{index}"),
                    "A one line description".to_owned(),
                )
            })
            .collect();
        let borrowed: Vec<_> = skills
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let (panel, vcx) = open_menu(cx, &borrowed, Some(vec![server("alpha", "local server")]));
        let count = values(&panel, vcx).len();
        let mut frames = Vec::new();
        for _ in 0..3 {
            let menu = vcx
                .debug_bounds("slash-command-overlay")
                .expect("menu drawn");
            let list = vcx
                .debug_bounds("slash-command-scroll")
                .expect("list drawn");
            let last = vcx.debug_bounds(Box::leak(
                format!("slash-command-row-{}", count - 1).into_boxed_str(),
            ));
            if let Some(last) = last.filter(|last| last.bottom() <= list.bottom()) {
                assert!(
                    list.bottom() - last.bottom() < px(2.),
                    "{rows} skills: {} of slack under the last row, so a height was estimated",
                    list.bottom() - last.bottom()
                );
            }
            frames.push((menu, list));
            vcx.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(
            frames.windows(2).all(|pair| pair[0] == pair[1]),
            "{rows} skills: {frames:?}"
        );
    }
}

#[gpui::test]
fn the_mcp_row_opens_the_configured_servers_and_escape_comes_back(cx: &mut gpui::TestAppContext) {
    let servers = vec![
        server("alpha", "2 tools: search, fetch"),
        server("beta", "disabled · local server"),
    ];
    let (panel, vcx) = open_menu(cx, &FOUR, Some(servers));
    vcx.run_until_parked();
    let all = values(&panel, vcx);
    let mcp = all
        .iter()
        .position(|row| row == "MCP")
        .expect("MCP row after Attach");
    assert_eq!(all[mcp - 1], "Attach…");
    let row = vcx
        .debug_bounds(Box::leak(
            format!("slash-command-row-{mcp}").into_boxed_str(),
        ))
        .unwrap();
    let chevron = vcx
        .debug_bounds("picker-submenu-chevron")
        .expect("a chevron on the MCP row");
    assert!(row.contains(&chevron.center()) && chevron.left() > row.center().x);

    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(values(&panel, vcx), ["Back", "alpha", "beta"]);
    assert!(
        vcx.debug_bounds("picker-help-1").is_some(),
        "tools shown beside the server"
    );

    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        values(&panel, vcx),
        all,
        "Esc leaves the submenu, not the menu"
    );
    vcx.simulate_keystrokes("escape");
    assert!(panel.read_with(vcx, |panel, _| panel.picker_search.is_none()));
}

#[gpui::test]
fn the_mcp_row_exists_only_once_a_server_is_known(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open_menu(cx, &FOUR, Some(Vec::new()));
    assert!(!values(&panel, vcx).contains(&"MCP".to_owned()));
    let (panel, vcx) = open_menu(cx, &FOUR, None);
    assert!(
        !values(&panel, vcx).contains(&"MCP".to_owned()),
        "nothing to list before the files are read"
    );
}
