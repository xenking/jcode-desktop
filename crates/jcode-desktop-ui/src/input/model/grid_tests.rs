//! The skill and mode picker draws each row as one fixed line, the name and
//! then its description, at any window width.
use super::*;
use super::model_picker::row_paint;

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
fn each_row_is_one_line_with_its_description_right_after_the_name(cx: &mut gpui::TestAppContext) {
    for width in [300., 360., 520., 900., 1400.] {
        for selected in [0, 2, 4] {
            let (_input, vcx) = open_picker(cx, width, selected);
            for index in 0..5 {
                let mut bounds = |selector: &str| {
                    vcx.debug_bounds(Box::leak(format!("{selector}-{index}").into_boxed_str()))
                        .unwrap_or_else(|| panic!("{selector} {index} painted at {width}"))
                };
                let (row, name, help) =
                    (bounds("slash-command-row"), bounds("picker-name"), bounds("picker-help"));
                assert_eq!(row.size.height, px(32.), "row {index} at {width} is not one line");
                assert!(help.right() <= row.right(), "clipped at {width}: {help:?} in {row:?}");
                assert!(
                    help.left() >= name.right() && help.left() - name.right() < px(40.),
                    "description {index} is not beside its name at {width}: {name:?} {help:?}"
                );
            }
        }
    }
}

#[test]
fn selecting_a_name_and_description_row_lightens_it_and_keeps_its_ink() {
    let theme = crate::theme::Theme::global();
    let (idle_fill, idle_ink) = row_paint(true, false, true, theme);
    let (fill, ink) = row_paint(true, true, true, theme);
    assert_eq!(ink, idle_ink, "the name keeps its colour under the selection");
    assert_ne!(ink, theme.BG, "no dark text on a bright bar");
    assert!(fill.a > idle_fill.a && fill.a < 0.2, "a faint wash, not a solid bar: {fill:?}");
    // Rows that still invert (models, efforts) are untouched.
    assert_eq!(row_paint(false, true, false, theme), (theme.TEXT, theme.BG));
}

#[gpui::test]
fn moving_the_selection_never_resizes_or_moves_the_list(cx: &mut gpui::TestAppContext) {
    // Row 0 carries a long description: no detail pane may open under it, and
    // no row may swap its text for a key hint when it takes the selection.
    let layout = |cx: &mut gpui::TestAppContext, selected: usize| {
        let (_input, vcx) = open_picker(cx, 420., selected);
        assert!(vcx.debug_bounds("picker-detail").is_none());
        let mut bounds = vec![vcx.debug_bounds("slash-command-scroll").unwrap()];
        for index in 0..5 {
            for selector in ["slash-command-row", "picker-help", "picker-name"] {
                let name = format!("{selector}-{index}");
                bounds.push(vcx.debug_bounds(Box::leak(name.into_boxed_str())).unwrap());
            }
        }
        bounds
    };
    let first = layout(cx, 0);
    for selected in 1..5 {
        assert_eq!(first, layout(cx, selected), "selection {selected} re-laid the list out");
    }
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
