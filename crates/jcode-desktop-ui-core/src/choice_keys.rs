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
    First,
    Last,
    /// A number key: the zero-based option it names.
    Pick(usize),
    /// Space: flip the highlighted option of a multi-select.
    Toggle,
    Confirm,
    /// Escape: leave a text field for the list, or dismiss the list.
    Dismiss,
    /// Tab from the list to its free-text field, Shift+Tab from the field back.
    /// The other direction of each is left to focus traversal, so the card's
    /// buttons stay reachable from the keyboard.
    SwitchField,
    /// Tab in a search field: take the highlighted row without confirming it.
    Complete,
    /// Left and Right: the previous or next question of a multi-question card.
    PrevGroup,
    NextGroup,
}

/// Digits 1 to 9 name options 1 to 9. A list longer than that numbers only its
/// first nine rows; the rest are reached with the arrows, Home and End.
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
        return match (scope, chord.shift) {
            (Scope::Search, _) => Some(ChoiceKey::Complete),
            // Tab walks on to the card's buttons, Shift+Tab walks back from them.
            (Scope::List, false) | (Scope::Text, true) => Some(ChoiceKey::SwitchField),
            (Scope::List, true) | (Scope::Text, false) => None,
        };
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
            "up" | "k" => Some(ChoiceKey::Prev),
            "down" | "j" => Some(ChoiceKey::Next),
            "home" => Some(ChoiceKey::First),
            "end" => Some(ChoiceKey::Last),
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

/// The row `delta` rows from `current` in a list of `len` rows. A card's list
/// wraps at its ends. A picker's list stops there: Up on the first row keeps it
/// highlighted, so a stray arrow never jumps a typed query to the far end.
pub fn step(scope: Scope, current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    match scope {
        Scope::Search => current.saturating_add_signed(delta).min(len - 1),
        Scope::List | Scope::Text => (current as isize + delta).rem_euclid(len as isize) as usize,
    }
}

/// The one-line key hint under a card's options.
pub fn hint(scope: Scope, multi_select: bool) -> &'static str {
    match (scope, multi_select) {
        (Scope::List, false) => {
            "↑↓ move · 1-9 pick · Enter confirm · Tab type your own · Esc dismiss"
        }
        (Scope::List, true) => {
            "↑↓ move · 1-9 or Space toggle · Enter confirm · Tab type your own · Esc dismiss"
        }
        (Scope::Text, _) => "Enter confirm · Shift+Tab or Esc back to the options",
        (Scope::Search, _) => "↑↓ move · Enter confirm · Tab complete · Esc close",
    }
}

// Option rows: the pure parts of how a card shows its options.
use gpui::{Pixels, px};

/// Descriptions longer than this are offered an expand toggle: at the card's
/// usual width one line holds about this much next to a label.
const INLINE_DESCRIPTION_CHARS: usize = 48;
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

/// Whether an option's description needs an expand toggle to be read whole.
pub fn expandable(description: &str) -> bool {
    description.trim().contains('\n') || description.chars().count() > INLINE_DESCRIPTION_CHARS
}

/// The first line of a description, for the collapsed row.
pub fn first_line(description: &str) -> &str {
    description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
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
    fn only_long_or_multi_line_descriptions_expand() {
        assert!(!expandable("Fits on a line"));
        assert!(expandable("Two steps\n\n- first\n- second"));
        assert!(expandable(&"x".repeat(49)));
        assert_eq!(first_line("\n  Lead line\nmore"), "Lead line");
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
    fn tab_goes_to_the_text_field_and_shift_tab_comes_back() {
        let shift = Chord {
            shift: true,
            ..chord("tab")
        };
        assert_eq!(
            classify(chord("tab"), Scope::List, 4),
            Some(ChoiceKey::SwitchField)
        );
        assert_eq!(
            classify(shift, Scope::Text, 4),
            Some(ChoiceKey::SwitchField)
        );
        // The other directions walk on to the card's buttons.
        assert_eq!(classify(shift, Scope::List, 4), None);
        assert_eq!(classify(chord("tab"), Scope::Text, 4), None);
        assert_eq!(
            classify(chord("tab"), Scope::Search, 4),
            Some(ChoiceKey::Complete)
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
    fn a_card_wraps_at_its_ends_and_a_picker_stops() {
        assert_eq!(step(Scope::List, 0, -1, 4), 3);
        assert_eq!(step(Scope::List, 3, 1, 4), 0);
        assert_eq!(step(Scope::List, 1, 1, 4), 2);
        assert_eq!(step(Scope::Search, 0, -1, 4), 0);
        assert_eq!(step(Scope::Search, 3, 1, 4), 3);
        assert_eq!(step(Scope::Search, 1, 1, 4), 2);
        assert_eq!(
            step(Scope::Search, 9, 1, 4),
            3,
            "a stale highlight is clamped"
        );
        assert_eq!(step(Scope::List, 0, 1, 0), 0);
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
