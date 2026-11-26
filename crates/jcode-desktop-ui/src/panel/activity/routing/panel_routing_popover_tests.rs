//! The routing popover shows the whole Why: wrapped, never cut, scrollable.
use super::*;
use gpui::{Bounds, Pixels};

const SHORT: &str = "priority 1";
const LONG: &str = "behind pace: claude:claude-otter would be at 91% of its weekly window by the reset, \
                    so the other subscription takes the next turns until the pace evens out";

fn open<'a>(
    cx: &'a mut gpui::TestAppContext,
    reason: &str,
    height: f32,
) -> (&'a mut gpui::VisualTestContext, Vec<Bounds<Pixels>>) {
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("session-a".into(), None, None, bridge, cx));
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(900.), px(height)));
    let decided = serde_json::from_value(serde_json::json!({
        "ev": "routing_decision", "session_id": "session-a", "account": "claude:claude-fox",
        "reason": reason, "warnings": [], "pending": null,
    }))
    .unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&decided, cx));
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("composer-routing-chip").expect("chip");
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let rows = (0..12)
        .map_while(|i| {
            vcx.debug_bounds(Box::leak(
                format!("routing-popover-row-{i}").into_boxed_str(),
            ))
        })
        .collect();
    (vcx, rows)
}

#[gpui::test]
fn a_long_why_wraps_inside_the_popover_and_the_next_row_starts_below_it(
    cx: &mut gpui::TestAppContext,
) {
    let (vcx, rows) = open(cx, LONG, 800.);
    let popover = vcx.debug_bounds("routing-popover").expect("popover");
    assert!(rows.len() >= 3, "{rows:?}");
    let why = rows
        .iter()
        .position(|row| row.size.height > px(20.))
        .expect("the Why row wraps onto several lines");
    for pair in rows.windows(2) {
        assert!(
            pair[0].bottom() <= pair[1].top() + px(0.5),
            "rows overlap: {pair:?}"
        );
    }
    assert!(why + 1 < rows.len(), "a row follows the Why");
    assert!(
        rows.iter()
            .all(|row| row.right() <= popover.right() && row.bottom() <= popover.bottom()),
        "{rows:?} in {popover:?}"
    );
}

#[gpui::test]
fn a_short_why_stays_on_one_line(cx: &mut gpui::TestAppContext) {
    let (_vcx, rows) = open(cx, SHORT, 800.);
    assert!(rows.len() >= 2);
    assert!(rows.iter().all(|row| row.size.height < px(17.)), "{rows:?}");
}

#[gpui::test]
fn a_why_taller_than_the_window_scrolls_inside_the_popover(cx: &mut gpui::TestAppContext) {
    let reason = LONG.repeat(3);
    let (vcx, rows) = open(cx, &reason, 220.);
    let popover = vcx.debug_bounds("routing-popover").expect("popover");
    let scroll = vcx
        .debug_bounds("routing-popover-rows")
        .expect("rows scroll");
    assert!(
        popover.top() >= px(0.) && popover.bottom() <= px(220.),
        "{popover:?}"
    );
    let content_bottom = rows.iter().map(|row| row.bottom()).max().unwrap();
    assert!(
        content_bottom > scroll.bottom(),
        "the rows overflow the popover and scroll: {rows:?} in {scroll:?}"
    );
}
