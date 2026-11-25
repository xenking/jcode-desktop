//! The skill and mode picker lays its rows out as one grid: names share a
//! column and every description starts at the same x, at any window width.
use super::*;

struct Fixture(Entity<PromptInput>);

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().pt(px(500.)).px(px(20.)).child(self.0.clone())
    }
}

const LONG: &str = "poteto's agent style for concise, detailed responses, deliberate subagents, \
                    unslopped prose, simple code, and verified work. Use for poteto or requests \
                    to work in this style.";

fn skills() -> Vec<(String, String)> {
    [
        ("poteto-mode", LONG),
        ("selfdev", "Develop the fork"),
        ("swarm", "Coordinate\nseveral workers"),
        ("go-review", LONG),
        ("clickhouse-architecture-advisor", "Design ClickHouse schemas"),
        ("quiet", "Brief"),
    ]
    .into_iter()
    .map(|(name, help)| (name.to_owned(), help.to_owned()))
    .collect()
}

fn open_picker(
    cx: &mut gpui::TestAppContext,
    width: f32,
    selected: usize,
) -> (Entity<PromptInput>, &mut gpui::VisualTestContext) {
    let (fixture, vcx) = cx.add_window_view(|_, cx| {
        Fixture(cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {})))
    });
    let input = fixture.read_with(vcx, |fixture, _| fixture.0.clone());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, size(px(width), px(800.)));
    input.update(vcx, |input, cx| {
        input.command_completion = true;
        input.set_skills(skills(), cx);
        input.set_current_modes(
            vec!["poteto-mode".into()],
            crate::mode_catalog::ModeCatalog::new(),
            cx,
        );
        input.set_content("/mode ".into(), cx);
        input.command_selection = selected;
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.run_until_parked();
    (input, vcx)
}

#[gpui::test]
fn descriptions_start_at_one_x_on_single_lines_at_every_width(cx: &mut gpui::TestAppContext) {
    for width in [300., 360., 520., 900., 1400.] {
        for selected in [0, 2, 4] {
            let (_input, vcx) = open_picker(cx, width, selected);
            let mut starts = Vec::new();
            let mut heights = Vec::new();
            for index in 0..5 {
                let row = vcx
                    .debug_bounds(Box::leak(format!("slash-command-row-{index}").into_boxed_str()))
                    .unwrap_or_else(|| panic!("row {index} painted at {width}"));
                let help = vcx
                    .debug_bounds(Box::leak(format!("picker-help-{index}").into_boxed_str()))
                    .unwrap_or_else(|| panic!("description {index} painted at {width}"));
                assert!(help.right() <= row.right(), "clipped at {width}: {help:?} in {row:?}");
                starts.push(help.left());
                heights.push(row.size.height);
            }
            assert!(
                starts.windows(2).all(|pair| pair[0] == pair[1]),
                "descriptions start at different x at width {width}, selected {selected}: {starts:?}"
            );
            assert!(
                heights.windows(2).all(|pair| pair[0] == pair[1]),
                "a description wrapped at width {width}, selected {selected}: {heights:?}"
            );
        }
    }
}

#[gpui::test]
fn the_focused_rows_full_text_is_shown_only_when_the_row_cannot_hold_it(
    cx: &mut gpui::TestAppContext,
) {
    // Row 0 carries a three-line description, row 1 a short one.
    let (_input, vcx) = open_picker(cx, 420., 0);
    let detail = vcx
        .debug_bounds("picker-detail")
        .expect("a truncated focused row opens the detail area");
    let list = vcx.debug_bounds("slash-command-scroll").unwrap();
    assert!(detail.top() >= list.bottom(), "{detail:?} under {list:?}");
    let (_input, vcx) = open_picker(cx, 420., 1);
    assert!(
        vcx.debug_bounds("picker-detail").is_none(),
        "a description that fits needs no detail area"
    );
}

#[gpui::test]
fn the_focused_rows_hint_never_moves_the_description_column(cx: &mut gpui::TestAppContext) {
    let column = |cx: &mut gpui::TestAppContext, selected: usize| {
        let (_input, vcx) = open_picker(cx, 600., selected);
        vcx.debug_bounds("picker-help-1").unwrap().left()
    };
    let unfocused = column(cx, 0);
    assert_eq!(unfocused, column(cx, 1));
    assert_eq!(unfocused, column(cx, 4));
}

#[gpui::test]
fn slash_palette_marks_modes_and_active_modes_among_skills(cx: &mut gpui::TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        input.command_completion = true;
        input.set_skills(skills(), cx);
        input.set_current_modes(
            vec!["poteto-mode".into()],
            crate::mode_catalog::ModeCatalog::new(),
            cx,
        );
        input.set_content("/".into(), cx);
        let rows = input.command_suggestions();
        let detail = |value: &str| {
            rows.iter()
                .find(|row| row.value == value)
                .unwrap_or_else(|| panic!("{value} listed"))
                .detail
                .clone()
        };
        assert_eq!(detail("/poteto-mode").as_deref(), Some("Active mode"));
        assert_eq!(detail("/selfdev").as_deref(), Some("Mode"));
        assert_eq!(detail("/go-review").as_deref(), Some("Skill"));
    });
}
