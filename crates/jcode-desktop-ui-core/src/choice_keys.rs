//! One key map for every list the user answers from the keyboard: ask cards,
//! MCP approval cards (an ask card) and the composer pickers.
//!
//! Callers translate a keystroke with [`classify`] and act on the [`ChoiceKey`]
//! it returns, so the same key means the same thing everywhere. The keys are
//! documented in `docs/choice-keys.md`.

/// Where keyboard focus sits when the key arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// A list of options holds focus (an ask card's rows).
    List,
    /// A free-text field next to the list holds focus; typing stays typing.
    Text,
    /// A search field drives the list under it (a composer picker): the field
    /// and the list share focus, so only the keys a text field leaves alone apply.
    Search,
}

/// What a key asks a list to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKey {
    Prev,
    Next,
    /// A number key: the zero-based option it names.
    Pick(usize),
    /// Space: flip the highlighted option of a multi-select.
    Toggle,
    Confirm,
    /// Escape: leave a text field for the list, close a picker, or hand a
    /// card's keys to the composer.
    Dismiss,
    /// Tab in a search field: take the highlighted row without confirming it.
    Complete,
    /// Left and Shift+Tab, Right and Tab: the previous or next question of a
    /// multi-question card. In a text field only Tab goes on; Shift+Tab is [`Self::Dismiss`].
    PrevGroup,
    NextGroup,
}

/// Digits 1 to 9 name options 1 to 9. A list longer than that numbers only its
/// first nine rows; the rest are reached with the arrows.
pub const MAX_NUMBERED: usize = 9;

/// One keystroke: its key name and whether Shift or any other modifier is down.
#[derive(Clone, Copy, Debug)]
pub struct Chord<'a> {
    pub key: &'a str,
    pub shift: bool,
    /// Control, Alt, Command or Fn: never part of this key map.
    pub other_modifier: bool,
}

/// What `chord` means to a list of `options` rows held in `scope`.
pub fn classify(chord: Chord<'_>, scope: Scope, options: usize) -> Option<ChoiceKey> {
    if chord.other_modifier {
        return None;
    }
    if chord.key == "tab" {
        return Some(match (scope, chord.shift) {
            (Scope::Search, _) => ChoiceKey::Complete,
            (Scope::List | Scope::Text, false) => ChoiceKey::NextGroup,
            (Scope::List, true) => ChoiceKey::PrevGroup,
            (Scope::Text, true) => ChoiceKey::Dismiss,
        });
    }
    if chord.shift {
        return None;
    }
    let key = chord.key;
    match scope {
        Scope::Text => match key {
            "enter" => Some(ChoiceKey::Confirm),
            "escape" => Some(ChoiceKey::Dismiss),
            _ => None,
        },
        Scope::Search => match key {
            "up" => Some(ChoiceKey::Prev),
            "down" => Some(ChoiceKey::Next),
            "enter" => Some(ChoiceKey::Confirm),
            "escape" => Some(ChoiceKey::Dismiss),
            _ => None,
        },
        Scope::List => match key {
            "up" => Some(ChoiceKey::Prev),
            "down" => Some(ChoiceKey::Next),
            "left" => Some(ChoiceKey::PrevGroup),
            "right" => Some(ChoiceKey::NextGroup),
            "space" => Some(ChoiceKey::Toggle),
            "enter" => Some(ChoiceKey::Confirm),
            "escape" => Some(ChoiceKey::Dismiss),
            digit => match digit.as_bytes() {
                [d @ b'1'..=b'9'] => {
                    let index = usize::from(d - b'1');
                    (index < options).then_some(ChoiceKey::Pick(index))
                }
                _ => None,
            },
        },
    }
}

/// The keystroke a gpui binding uses for `key` in a [`Scope::Search`] list, so
/// the picker's bindings come from this map rather than a second copy of it.
pub fn search_keystroke(key: ChoiceKey) -> &'static str {
    match key {
        ChoiceKey::Prev => "up",
        ChoiceKey::Next => "down",
        ChoiceKey::Confirm => "enter",
        ChoiceKey::Dismiss => "escape",
        ChoiceKey::Complete => "tab",
        other => unreachable!("{other:?} has no key in a search list"),
    }
}

/// The row `delta` rows from `current` in a list of `len` rows. The list stops
/// at its ends: Up on the first row keeps it highlighted, so a stray arrow never
/// jumps a typed query to the far end.
pub fn step(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(len - 1)
}

// Option rows: the pure parts of how a card shows its options.
use gpui::{Pixels, px};

const RECOMMENDED: &str = "(Recommended)";

/// The option the agent recommends, by the `(Recommended)` label suffix every
/// ask prompt asks for. The wire has no separate flag.
pub fn recommended_option<'a>(labels: impl IntoIterator<Item = &'a str>) -> Option<usize> {
    labels
        .into_iter()
        .position(|label| label.trim_end().ends_with(RECOMMENDED))
}

/// An option's label as shown, and whether it was recommended: the suffix
/// becomes a badge.
pub fn shown_label(label: &str) -> (&str, bool) {
    match label.trim_end().strip_suffix(RECOMMENDED) {
        Some(label) => (label.trim_end(), true),
        None => (label, false),
    }
}

/// The scroll offset that brings a row into a viewport, moving only as far as needed.
/// `offset` is the scroll handle's (zero or negative) and `max` its largest scroll distance.
pub fn reveal_offset(
    (view_top, view_bottom): (Pixels, Pixels),
    (row_top, row_bottom): (Pixels, Pixels),
    offset: Pixels,
    max: Pixels,
) -> Pixels {
    let moved = if row_top < view_top {
        offset + (view_top - row_top)
    } else if row_bottom > view_bottom {
        offset - (row_bottom - view_bottom)
    } else {
        offset
    };
    moved.clamp(-max, px(0.))
}

#[cfg(test)]
mod row_tests {
    use super::*;

    #[test]
    fn recommended_options_are_found_by_their_label_suffix() {
        assert_eq!(
            recommended_option(["Plain", "Keep it (Recommended)"]),
            Some(1)
        );
        assert_eq!(recommended_option(["Plain", "Other"]), None);
        assert_eq!(shown_label("Keep it (Recommended)"), ("Keep it", true));
        assert_eq!(shown_label("Plain"), ("Plain", false));
    }

    #[test]
    fn the_cursor_row_scrolls_into_view_by_the_least_distance() {
        let reveal = |view: (f32, f32), row: (f32, f32), offset: f32, max: f32| {
            f32::from(reveal_offset(
                (px(view.0), px(view.1)),
                (px(row.0), px(row.1)),
                px(offset),
                px(max),
            ))
        };
        assert_eq!(reveal((100., 200.), (120., 150.), -30., 300.), -30.);
        assert_eq!(reveal((100., 200.), (190., 230.), -30., 300.), -60.);
        assert_eq!(reveal((100., 200.), (80., 110.), -10., 300.), 0.);
        assert_eq!(reveal((100., 200.), (190., 400.), -280., 300.), -300.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(key: &str) -> Chord<'_> {
        Chord {
            key,
            shift: false,
            other_modifier: false,
        }
    }

    #[test]
    fn number_keys_pick_the_first_nine_options_only() {
        for count in [3, 9, 12] {
            for digit in 1..=9usize {
                let key = digit.to_string();
                let expected = (digit <= count).then_some(ChoiceKey::Pick(digit - 1));
                assert_eq!(classify(chord(&key), Scope::List, count), expected);
            }
        }
        // Past nine there is no digit for the tenth row; zero is not one either.
        assert_eq!(classify(chord("0"), Scope::List, 12), None);
        assert_eq!(classify(chord("10"), Scope::List, 12), None);
    }

    #[test]
    fn digits_are_typing_in_a_text_field_and_a_search_field() {
        for scope in [Scope::Text, Scope::Search] {
            assert_eq!(classify(chord("2"), scope, 4), None);
            assert_eq!(classify(chord("j"), scope, 4), None);
        }
    }

    #[test]
    fn tab_walks_the_questions_and_shift_tab_leaves_a_text_field() {
        let shift = Chord {
            shift: true,
            ..chord("tab")
        };
        assert_eq!(
            classify(chord("tab"), Scope::List, 4),
            Some(ChoiceKey::NextGroup)
        );
        assert_eq!(classify(shift, Scope::List, 4), Some(ChoiceKey::PrevGroup));
        assert_eq!(
            classify(chord("tab"), Scope::Text, 4),
            Some(ChoiceKey::NextGroup)
        );
        assert_eq!(classify(shift, Scope::Text, 4), Some(ChoiceKey::Dismiss));
        assert_eq!(
            classify(chord("tab"), Scope::Search, 4),
            Some(ChoiceKey::Complete)
        );
    }

    #[test]
    fn only_the_keys_of_the_reference_move_a_card() {
        for key in ["j", "k", "home", "end"] {
            assert_eq!(classify(chord(key), Scope::List, 4), None, "{key}");
        }
        assert_eq!(
            classify(chord("left"), Scope::List, 4),
            Some(ChoiceKey::PrevGroup)
        );
        assert_eq!(
            classify(chord("right"), Scope::List, 4),
            Some(ChoiceKey::NextGroup)
        );
    }

    #[test]
    fn enter_and_escape_mean_the_same_in_every_scope() {
        for scope in [Scope::List, Scope::Text, Scope::Search] {
            assert_eq!(classify(chord("enter"), scope, 4), Some(ChoiceKey::Confirm));
            assert_eq!(
                classify(chord("escape"), scope, 4),
                Some(ChoiceKey::Dismiss)
            );
        }
        // Shift+Enter is a newline in a text field, never a confirm.
        let newline = Chord {
            shift: true,
            ..chord("enter")
        };
        assert_eq!(classify(newline, Scope::Text, 4), None);
    }

    #[test]
    fn modifier_chords_are_left_to_their_own_bindings() {
        let command = Chord {
            other_modifier: true,
            ..chord("up")
        };
        for scope in [Scope::List, Scope::Text, Scope::Search] {
            assert_eq!(classify(command, scope, 4), None);
        }
    }

    #[test]
    fn a_list_stops_at_its_ends() {
        assert_eq!(step(0, -1, 4), 0);
        assert_eq!(step(3, 1, 4), 3);
        assert_eq!(step(1, 1, 4), 2);
        assert_eq!(step(9, 1, 4), 3, "a stale highlight is clamped");
        assert_eq!(step(0, 1, 0), 0);
    }

    #[test]
    fn picker_bindings_agree_with_the_search_map() {
        for key in [
            ChoiceKey::Prev,
            ChoiceKey::Next,
            ChoiceKey::Confirm,
            ChoiceKey::Dismiss,
            ChoiceKey::Complete,
        ] {
            assert_eq!(
                classify(chord(search_keystroke(key)), Scope::Search, 4),
                Some(key)
            );
        }
    }
}
