use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Role, TestAppContext,
    Window, div, px,
};

use super::{Pill, PillVariant, Tooltip};

/// Renders one pill with a click counter and an optional focus handle.
struct Harness {
    clicks: Rc<Cell<usize>>,
    focus: FocusHandle,
    disabled: bool,
}

impl Harness {
    fn new(cx: &mut Context<Self>, disabled: bool) -> (Self, Rc<Cell<usize>>) {
        let clicks = Rc::new(Cell::new(0));
        let harness = Self {
            clicks: clicks.clone(),
            focus: cx.focus_handle(),
            disabled,
        };
        (harness, clicks)
    }
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div().size(px(400.)).child(
            Pill::primary("pill", "Save")
                .debug_selector("pill")
                .track_focus(&self.focus)
                .disabled(self.disabled)
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
        )
    }
}

fn harness(cx: &mut TestAppContext, disabled: bool) -> (gpui::VisualTestContext, Rc<Cell<usize>>) {
    let mut clicks = None;
    let (view, vcx) = cx.add_window_view(|_, cx| {
        let (harness, shared) = Harness::new(cx, disabled);
        clicks = Some(shared);
        harness
    });
    let mut vcx = vcx.clone();
    let handle = view.read_with(&vcx, |harness, _| harness.focus.clone());
    vcx.update(|window, cx| handle.focus(window, cx));
    (vcx, clicks.unwrap())
}

/// A full key press. GPUI fires a keyboard click on key-up, after a key-down
/// on the same focused element.
fn press(vcx: &mut gpui::VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    vcx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    vcx.simulate_event(KeyUpEvent { keystroke });
}

#[test]
fn a_pill_is_a_button_named_by_its_label() {
    let pill = Pill::secondary("p", "Open").on_click(|_, _, _| {});
    let semantics = pill.semantics();
    assert_eq!(semantics.role, Role::Button);
    assert_eq!(semantics.label.as_ref(), "Open");
    assert!(semantics.actionable && semantics.focusable);
}

#[test]
fn an_explicit_aria_label_replaces_the_visible_label() {
    let semantics = Pill::compact("p", "×").aria_label("Close tab").semantics();
    assert_eq!(semantics.label.as_ref(), "Close tab");
}

#[test]
fn an_icon_button_is_named_by_its_aria_label() {
    let semantics = Pill::unlabeled(PillVariant::Ghost, "p", "Search").semantics();
    assert_eq!(semantics.role, Role::Button);
    assert_eq!(semantics.label.as_ref(), "Search");
}

#[test]
fn a_disabled_pill_is_neither_actionable_nor_focusable() {
    let semantics = Pill::primary("p", "Save")
        .disabled(true)
        .on_click(|_, _, _| {})
        .semantics();
    assert!(!semantics.actionable);
    assert!(!semantics.focusable);
    assert_eq!(semantics.label.as_ref(), "Save", "still announced");
}

#[test]
fn a_pill_without_a_handler_is_not_actionable() {
    assert!(!Pill::ghost("p", "Info").semantics().actionable);
}

#[test]
fn every_variant_is_a_button() {
    for variant in [
        PillVariant::Primary,
        PillVariant::Secondary,
        PillVariant::Danger,
        PillVariant::Compact,
        PillVariant::Ghost,
    ] {
        assert_eq!(Pill::new(variant, "p", "x").semantics().role, Role::Button);
    }
}

#[gpui::test]
fn clicking_runs_the_handler(cx: &mut TestAppContext) {
    let (mut vcx, clicks) = harness(cx, false);
    let bounds = vcx.debug_bounds("pill").expect("pill is laid out");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    assert_eq!(clicks.get(), 1);
}

#[gpui::test]
fn enter_and_space_activate_the_focused_pill(cx: &mut TestAppContext) {
    let (mut vcx, clicks) = harness(cx, false);
    press(&mut vcx, "enter");
    assert_eq!(clicks.get(), 1, "Enter activates");
    press(&mut vcx, "space");
    assert_eq!(clicks.get(), 2, "Space activates");
}

#[gpui::test]
fn a_disabled_pill_ignores_clicks_and_keys(cx: &mut TestAppContext) {
    let (mut vcx, clicks) = harness(cx, true);
    let bounds = vcx.debug_bounds("pill").expect("pill is laid out");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    press(&mut vcx, "enter");
    press(&mut vcx, "space");
    assert_eq!(clicks.get(), 0);
}

/// Lays a tooltip out at its natural width, as a hover layer does.
struct Holder(Entity<Tooltip>);

impl Render for Holder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().child(self.0.clone())
    }
}

fn holder(cx: &mut TestAppContext, tooltip: Tooltip) -> gpui::VisualTestContext {
    let (_, vcx) = cx.add_window_view(|_, cx| Holder(cx.new(|_| tooltip)));
    vcx.clone()
}

#[gpui::test]
fn a_text_tooltip_is_one_laid_out_block(cx: &mut TestAppContext) {
    let mut vcx = holder(cx, Tooltip::text("Rename").debug_selector("tt"));
    assert!(vcx.debug_bounds("tt").is_some());
}

#[gpui::test]
fn a_key_hint_makes_the_tooltip_wider(cx: &mut TestAppContext) {
    let mut plain = holder(cx, Tooltip::text("Rename").debug_selector("tt"));
    let plain_width = plain.debug_bounds("tt").unwrap().size.width;
    let mut hinted = holder(
        cx,
        Tooltip::text("Rename").key_hint("F2").debug_selector("tt"),
    );
    assert!(hinted.debug_bounds("tt").unwrap().size.width > plain_width);
}

#[gpui::test]
fn a_rows_tooltip_lays_out_a_title_and_each_row(cx: &mut TestAppContext) {
    let mut vcx = holder(
        cx,
        Tooltip::rows("Build", ["Version: 1", "Commit: abc"]).debug_selector("tt"),
    );
    for selector in ["tt", "tt-title", "tt-row-0", "tt-row-1"] {
        assert!(vcx.debug_bounds(selector).is_some(), "{selector}");
    }
    assert!(vcx.debug_bounds("tt-row-2").is_none());
}

#[gpui::test]
fn the_builder_makes_a_view_per_hover(cx: &mut TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, _| Tooltip::text("root"));
    vcx.update(|window, cx| {
        let build = Tooltip::text("hello").key_hint("F2").builder();
        let _first = build(window, cx);
        let _second = build(window, cx);
    });
}
