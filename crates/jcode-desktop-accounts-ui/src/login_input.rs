//! Ephemeral secret input. Only bullets reach GPUI's text layout and input readback.
//! No history, undo, persistence, copy, or cut support. This is not locked memory.
use crate::theme::{Theme, to_hsla};
use gpui::{
    App, Bounds, Context, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, GlobalElementId, LayoutId, MouseButton, PaintQuad, Pixels, SharedString, Style,
    TextRun, UTF16Selection, Window, WrappedLine, div, fill, point, prelude::*, px, relative, size,
};
use jcode_desktop_model::text_field::{
    FieldSelection, byte_to_utf16, next_char_boundary, previous_char_boundary, range_from_utf16,
    range_to_utf16, utf16_to_byte,
};
use jcode_desktop_ui_core::ime_geometry::ImeGeometry;
use std::ops::Range;

#[derive(Default)]
struct Secret {
    text: String,
    sel: FieldSelection,
}
impl Secret {
    fn move_to(&mut self, offset: usize, extend: bool) {
        if extend {
            self.sel.extend_to(offset);
        } else {
            self.sel.collapse_to(offset);
        }
    }
    fn masked(&self) -> String {
        "•".repeat(self.text.encode_utf16().count())
    }
    fn replace(&mut self, range: Range<usize>, text: &str) {
        self.text.replace_range(range.clone(), text);
        self.sel.commit(range.start, text.len());
    }
    fn take(&mut self) -> String {
        self.sel = FieldSelection::default();
        std::mem::take(&mut self.text)
    }
    fn clear(&mut self) {
        self.take().into_bytes().fill(0);
    }
}

pub struct LoginInput {
    pub focus_handle: FocusHandle,
    secret: Secret,
    placeholder: SharedString,
    last_layout: Option<WrappedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    ime_geometry: Option<ImeGeometry>,
}
impl LoginInput {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<String>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            secret: Secret::default(),
            placeholder: placeholder.into().into(),
            last_layout: None,
            last_bounds: None,
            ime_geometry: None,
        }
    }
    pub fn content_empty(&self) -> bool {
        self.secret.text.is_empty()
    }
    pub fn take(&mut self, cx: &mut Context<Self>) -> String {
        let text = self.secret.take();
        self.last_layout = None;
        cx.notify();
        text
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.secret.clear();
        self.last_layout = None;
        cx.notify();
    }
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
        self.focus_handle.focus(window, cx);
    }
    fn key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform {
            match key {
                "v" => self.paste(window, cx),
                "a" => {
                    self.secret.sel.select_all(self.secret.text.len());
                }
                // Explicitly swallow secret extraction and history commands.
                "c" | "x" | "z" | "y" => {}
                _ => return,
            }
        } else {
            let range = self.secret.sel.range.clone();
            let cursor = self.secret.sel.cursor();
            let previous = previous_char_boundary(&self.secret.text, cursor);
            let next = next_char_boundary(&self.secret.text, cursor);
            match key {
                "backspace" => self.secret.replace(
                    if range.is_empty() {
                        previous..range.end
                    } else {
                        range
                    },
                    "",
                ),
                "delete" => self.secret.replace(
                    if range.is_empty() {
                        range.start..next
                    } else {
                        range
                    },
                    "",
                ),
                "left" | "home" => {
                    let offset = if key == "home" {
                        0
                    } else if range.is_empty() || modifiers.shift {
                        previous
                    } else {
                        range.start
                    };
                    self.secret.move_to(offset, modifiers.shift);
                }
                "right" | "end" => {
                    let offset = if key == "end" {
                        self.secret.text.len()
                    } else if range.is_empty() || modifiers.shift {
                        next
                    } else {
                        range.end
                    };
                    self.secret.move_to(offset, modifiers.shift);
                }
                _ => return,
            }
        }
        self.secret.sel.marked = None;
        cx.stop_propagation();
        cx.notify();
    }
}
impl Drop for LoginInput {
    fn drop(&mut self) {
        self.secret.clear();
    }
}
impl Focusable for LoginInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
impl EntityInputHandler for LoginInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = range_to_utf16(
            &self.secret.text,
            &range_from_utf16(&self.secret.text, &range),
        );
        *actual = Some(range.clone());
        Some("•".repeat(range.len()))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: range_to_utf16(&self.secret.text, &self.secret.sel.range),
            reversed: self.secret.sel.reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.secret
            .sel
            .marked
            .as_ref()
            .map(|range| range_to_utf16(&self.secret.text, range))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.secret.sel.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self
            .secret
            .sel
            .replacement_range(&self.secret.text, range.as_ref());
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        self.secret.replace(range, &text);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self
            .secret
            .sel
            .replacement_range(&self.secret.text, range.as_ref());
        let start = range.start;
        self.secret.replace(range, text);
        self.secret.sel.mark(start, text, selected.as_ref());
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = range_to_utf16(
            &self.secret.text,
            &range_from_utf16(&self.secret.text, &range),
        );
        self.ime_geometry.as_ref()?.bounds_for_range(
            self.secret.masked().into(),
            range.start * "•".len()..range.end * "•".len(),
            bounds.origin,
            window,
        )
    }
    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.last_bounds?.localize(&point)?;
        let index = self
            .last_layout
            .as_ref()?
            .closest_index_for_position(local, window.line_height())
            .unwrap_or_else(|i| i);
        Some((index / "•".len()).min(self.secret.text.encode_utf16().count()))
    }
}

struct TextElement {
    input: Entity<LoginInput>,
}

struct PrepaintState {
    ime_geometry: ImeGeometry,
    line: Option<WrappedLine>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    text_bounds: Bounds<Pixels>,
}

fn selection_quads(
    line: &WrappedLine,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<PaintQuad> {
    let Some(start) = line.position_for_index(range.start, line_height) else {
        return Vec::new();
    };
    let Some(end) = line.position_for_index(range.end, line_height) else {
        return Vec::new();
    };
    let first_row = (start.y / line_height) as usize;
    let last_row = (end.y / line_height) as usize;
    (first_row..=last_row)
        .map(|row| {
            let left = if row == first_row { start.x } else { px(0.) };
            let right = if row == last_row {
                end.x
            } else {
                bounds.size.width
            };
            fill(
                Bounds::new(
                    point(bounds.left() + left, bounds.top() + line_height * row),
                    size((right - left).max(px(0.)), line_height),
                ),
                to_hsla(Theme::global().SELECTION),
            )
        })
        .collect()
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let line_count = 1;
        style.size.height = (window.line_height() * line_count).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content: SharedString = input.secret.masked().into();
        let text = &input.secret.text;
        let selected = range_to_utf16(text, &input.secret.sel.range);
        let selected_range = selected.start * "•".len()..selected.end * "•".len();
        let cursor = byte_to_utf16(text, input.secret.sel.cursor()) * "•".len();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), to_hsla(Theme::global().TEXT_DIM))
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = vec![run];

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let ime_geometry = ImeGeometry::new(runs[0].font.clone(), font_size, line_height, None);
        let line = window
            .text_system()
            .shape_text(display_text, font_size, &runs, None, None)
            .expect("masked login text should shape")
            .into_iter()
            .next()
            .expect("shape_text always returns a line");
        let cursor_pos = line
            .position_for_index(cursor, line_height)
            .unwrap_or_default();
        let visual_line_count = line.wrap_boundaries().len() + 1;
        let text_bounds = Bounds::new(
            bounds.origin,
            size(bounds.size.width, line_height * visual_line_count),
        );
        let (selection, cursor) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(text_bounds.origin + cursor_pos, size(px(2.), line_height)),
                    to_hsla(Theme::global().CURSOR),
                )),
            )
        } else {
            (
                selection_quads(&line, selected_range, text_bounds, line_height),
                None,
            )
        };
        PrepaintState {
            ime_geometry,
            line: Some(line),
            cursor,
            selection,
            text_bounds,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            prepaint.text_bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.ime_geometry = Some(prepaint.ime_geometry.clone());
            input.last_layout = Some(line);
            input.last_bounds = Some(prepaint.text_bounds);
        });
    }
}

impl Render for LoginInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("LoginInput");
        div()
            .id("login-input")
            .key_context("LoginInput")
            .track_focus(&self.focus_handle)
            .w_full()
            .px_3()
            .py_2()
            .overflow_hidden()
            .cursor_text()
            .bg(to_hsla(Theme::global().BG))
            .text_color(to_hsla(Theme::global().TEXT))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    this.focus_handle.focus(window, cx);
                    if let Some(offset) = this.character_index_for_point(event.position, window, cx)
                    {
                        let offset = utf16_to_byte(&this.secret.text, offset);
                        this.secret.move_to(offset, false);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(TextElement { input: cx.entity() })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn masked_ime_bounds_follow_preedit_without_paint(cx: &mut gpui::TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|cx| LoginInput::new(cx, "API key"))
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "x", window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, window, cx| {
                let bounds = input.last_bounds.unwrap();
                let anchor = input.bounds_for_range(1..1, bounds, window, cx).unwrap();
                let mut widths = Vec::new();
                for marked in ["a", "abcde", "😀", "ab", "W"] {
                    let end = marked.encode_utf16().count();
                    input.replace_and_mark_text_in_range(None, marked, Some(end..end), window, cx);
                    let rect = input
                        .bounds_for_range(1..1 + end, bounds, window, cx)
                        .expect("masked preedit must have bounds before the next paint");
                    assert_eq!(rect.origin, anchor.origin);
                    assert_eq!(rect.size.height, anchor.size.height);
                    widths.push(rect.size.width);
                }
                assert!(widths[0] > px(0.));
                assert!(widths[1] > widths[2]);
                assert!(widths[2] > widths[0]);
                assert_eq!(
                    widths[2], widths[3],
                    "one emoji and two ASCII letters use two bullets"
                );
                assert_eq!(
                    widths[0], widths[4],
                    "ASCII glyph widths must not reveal the secret"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn native_input_masks_layout_and_readback_and_clears(cx: &mut gpui::TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|cx| LoginInput::new(cx, "API key"))
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();
        cx.simulate_input(*window, "secret-😀");
        cx.run_until_parked();
        window
            .update(cx, |input, window, cx| {
                assert!(!input.content_empty());
                assert_eq!(input.secret.masked(), "•".repeat(9));
                assert_eq!(
                    input.last_layout.as_ref().unwrap().text.as_ref(),
                    "•".repeat(9)
                );
                let mut actual = None;
                assert_eq!(
                    input.text_for_range(0..99, &mut actual, window, cx),
                    Some("•".repeat(9))
                );
                assert_eq!(actual, Some(0..9));
                assert_eq!(input.take(cx), "secret-😀");
                assert!(input.content_empty());
                input.replace_and_mark_text_in_range(None, "😀ab", Some(2..3), window, cx);
                assert_eq!(input.secret.sel.range, 4..5);
                input.clear(cx);
                assert!(input.content_empty());
                assert!(input.secret.sel.marked.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn keyboard_editing_and_clipboard_do_not_export_secret(cx: &mut gpui::TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|cx| LoginInput::new(cx, "API key"))
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();
        cx.simulate_input(*window, "abcd");
        let mut cx = gpui::VisualTestContext::from_window(*window, cx);
        cx.simulate_keystrokes("shift-left shift-left shift-right backspace");
        cx.run_until_parked();
        window
            .update(&mut cx, |input, _, cx| {
                assert_eq!(input.secret.text, "abc");
                cx.write_to_clipboard(gpui::ClipboardItem::new_string("clipboard".into()));
            })
            .unwrap();
        cx.simulate_keystrokes("ctrl-a ctrl-c ctrl-x ctrl-z");
        cx.run_until_parked();
        window
            .update(&mut cx, |input, window, cx| {
                assert_eq!(input.secret.text, "abc");
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some("clipboard")
                );
                input.paste(window, cx);
                assert_eq!(input.take(cx), "clipboard");
            })
            .unwrap();
    }

    // ---- Characterization pins -------------------------------------------
    //
    // Pinned before the caret, selection and IME rules moved into
    // jcode-desktop-model::text_field. They read the field through the platform
    // input handler where possible so they survive the move.

    fn login_window(cx: &mut gpui::TestAppContext) -> gpui::WindowHandle<LoginInput> {
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|cx| LoginInput::new(cx, "API key"))
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();
        window
    }

    #[gpui::test]
    fn arrow_keys_step_by_scalar_and_extend_around_the_anchor(cx: &mut gpui::TestAppContext) {
        let window = login_window(cx);
        // `e` + combining acute is one grapheme but two scalars.
        cx.simulate_input(*window, "abé😀e\u{301}");
        let mut cx = gpui::VisualTestContext::from_window(*window, cx);
        cx.simulate_keystrokes("left");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.sel.range, 9..9, "left steps over one scalar");
            })
            .unwrap();
        cx.simulate_keystrokes("left left");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.sel.range, 4..4, "emoji is one step");
            })
            .unwrap();
        cx.simulate_keystrokes("shift-right shift-right");
        window
            .update(&mut cx, |input, window, cx| {
                assert_eq!(input.secret.sel.range, 4..9);
                let selected = input.selected_text_range(false, window, cx).unwrap();
                assert_eq!(selected.range, 3..6);
                assert!(!selected.reversed);
            })
            .unwrap();
        cx.simulate_keystrokes("shift-left shift-left shift-left");
        window
            .update(&mut cx, |input, window, cx| {
                assert_eq!(input.secret.sel.range, 2..4, "the anchor stays at 4");
                let selected = input.selected_text_range(false, window, cx).unwrap();
                assert!(selected.reversed);
            })
            .unwrap();
        // A plain arrow collapses to the matching edge.
        cx.simulate_keystrokes("right");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.sel.range, 4..4);
            })
            .unwrap();
        cx.simulate_keystrokes("home shift-end");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.sel.range, 0..input.secret.text.len());
            })
            .unwrap();
    }

    #[gpui::test]
    fn backspace_and_delete_remove_one_scalar_or_the_selection(cx: &mut gpui::TestAppContext) {
        let window = login_window(cx);
        cx.simulate_input(*window, "a😀e\u{301}b");
        let mut cx = gpui::VisualTestContext::from_window(*window, cx);
        cx.simulate_keystrokes("left left backspace");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.text, "a😀\u{301}b", "one scalar, not the cluster");
                assert_eq!(input.secret.sel.range, 5..5);
            })
            .unwrap();
        cx.simulate_keystrokes("delete delete");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.text, "a😀");
            })
            .unwrap();
        cx.simulate_keystrokes("delete home right shift-right backspace");
        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(input.secret.text, "a");
                assert_eq!(input.secret.sel.range, 1..1);
            })
            .unwrap();
        cx.simulate_keystrokes("backspace backspace delete");
        window
            .update(&mut cx, |input, _, _| {
                assert!(input.content_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn ime_marked_text_replaces_in_place_and_selects_relative_to_it(cx: &mut gpui::TestAppContext) {
        let window = login_window(cx);
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "你好，", window, cx);
                let start = "你好，".len();
                for marked in ["n", "ni", "ni h"] {
                    let caret = marked.encode_utf16().count();
                    input.replace_and_mark_text_in_range(
                        None,
                        marked,
                        Some(caret..caret),
                        window,
                        cx,
                    );
                    assert_eq!(input.secret.text, format!("你好，{marked}"));
                    assert_eq!(input.secret.sel.marked, Some(start..start + marked.len()));
                    assert_eq!(
                        input.secret.sel.range,
                        start + marked.len()..start + marked.len()
                    );
                    assert_eq!(
                        input.marked_text_range(window, cx),
                        Some(3..3 + caret),
                        "reported in UTF-16 units"
                    );
                }
                // The IME selection is relative to the new text and clamped.
                input.replace_and_mark_text_in_range(None, "你好", Some(1..9), window, cx);
                assert_eq!(input.secret.text, "你好，你好");
                assert_eq!(input.secret.sel.range, start + 3..start + 6);
                // Committing without a range replaces the marked text.
                input.replace_text_in_range(None, "你好🚀", window, cx);
                assert_eq!(input.secret.text, "你好，你好🚀");
                assert!(input.secret.sel.marked.is_none());
                assert_eq!(
                    input.secret.sel.range,
                    input.secret.text.len()..input.secret.text.len()
                );
                // An empty marked string clears the composition.
                input.replace_and_mark_text_in_range(None, "x", None, window, cx);
                assert!(input.secret.sel.marked.is_some());
                input.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert!(input.secret.sel.marked.is_none());
                assert_eq!(input.secret.text, "你好，你好🚀");
            })
            .unwrap();
    }

    #[gpui::test]
    fn platform_ranges_clamp_and_floor_inside_a_surrogate_pair(cx: &mut gpui::TestAppContext) {
        let window = login_window(cx);
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "a😀b", window, cx);
                let mut actual = None;
                // Offset 2 is inside the emoji and rounds down to its start.
                let text = input.text_for_range(0..2, &mut actual, window, cx);
                assert_eq!(actual, Some(0..1));
                assert_eq!(text.as_deref(), Some("•"));
                input.text_for_range(1..99, &mut actual, window, cx);
                assert_eq!(actual, Some(1..4));
                // Inverted ranges are ordered rather than sliced blindly.
                #[allow(clippy::reversed_empty_ranges)]
                let inverted = 3..1;
                input.replace_text_in_range(Some(inverted), "x", window, cx);
                assert_eq!(input.secret.text, "a😀xb");
                input.replace_text_in_range(Some(0..1), "", window, cx);
                assert_eq!(input.secret.text, "😀xb");
            })
            .unwrap();
    }

    #[gpui::test]
    fn committed_text_drops_control_characters_but_marked_text_keeps_them(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = login_window(cx);
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "a\tb\r\nc", window, cx);
                assert_eq!(input.secret.text, "abc");
                input.replace_and_mark_text_in_range(None, "\t", None, window, cx);
                assert_eq!(input.secret.text, "abc\t");
            })
            .unwrap();
    }

    #[test]
    fn secret_is_never_display_text() {
        let mut secret = Secret::default();
        secret.replace(0..0, "private-token-😀é");
        let display = secret.masked();
        assert!(display.chars().all(|ch| ch == '•'));
        assert_eq!(display.chars().count(), secret.text.encode_utf16().count());
        assert!(!display.contains("private-token"));
    }
    #[test]
    fn take_and_clear_remove_content_and_composition() {
        let mut secret = Secret::default();
        secret.replace(0..0, "secret");
        secret.sel.marked = Some(0..6);
        assert_eq!(secret.take(), "secret");
        assert!(secret.text.is_empty());
        assert!(secret.masked().is_empty());
        assert_eq!(secret.sel.range, 0..0);
        assert_eq!(secret.sel.marked, None);
        secret.replace(0..0, "another");
        secret.clear();
        assert!(secret.text.is_empty());
        assert!(secret.masked().is_empty());
    }
}
