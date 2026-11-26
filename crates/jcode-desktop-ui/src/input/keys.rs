//! The prompt's actions and key bindings.
use super::*;
use crate::choice_keys::{ChoiceKey, search_keystroke as search_key};

actions!(
    prompt_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        DecreasePickerSpeed,
        IncreasePickerSpeed,
        SelectAll,
        Home,
        End,
        Paste,
        PastePlainText,
        Cut,
        Copy,
        MoveWordLeft,
        MoveWordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordBack,
        DeleteWordForward,
        KillToStart,
        KillToEnd,
        Undo,
        Redo,
        HistoryPrev,
        HistoryNext,
        GrowEditor,
        ShrinkEditor,
        Clear,
        Stash,
        Submit,
        Queue,
        InsertNewline,
        ShiftSpace,
        AcceptCompletion,
        AlternateContext,
    ]
);

pub fn bind_keys(cx: &mut App) {
    crate::text_selection::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new(
            "alt-enter",
            AlternateContext,
            Some("PromptInput && ContextPicker"),
        ),
        KeyBinding::new("backspace", Backspace, Some("PromptInput")),
        // GPUI matches modifiers exactly, so Shift+Backspace needs its own
        // binding or it falls through to the platform and is dropped.
        KeyBinding::new("shift-backspace", Backspace, Some("PromptInput")),
        KeyBinding::new("delete", Delete, Some("PromptInput")),
        KeyBinding::new("left", Left, Some("PromptInput")),
        KeyBinding::new("right", Right, Some("PromptInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("PromptInput")),
        KeyBinding::new("shift-right", SelectRight, Some("PromptInput")),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-right", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("alt-left", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-right", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("alt-b", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-f", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("ctrl-b", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-f", MoveWordRight, Some("PromptInput")),
    ]);
    // Must follow the composer's Alt+B/Ctrl+B word motions: later bindings win
    // at equal context depth, and the handler falls through when idle.
    cx.bind_keys(crate::ui_actions::background_tool_bindings());
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("PromptInput")),
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-shift-right", SelectWordRight, Some("PromptInput")),
        KeyBinding::new(
            "alt-shift-left",
            DecreasePickerSpeed,
            Some("PromptInput && ModelPicker"),
        ),
        KeyBinding::new(
            "alt-shift-right",
            IncreasePickerSpeed,
            Some("PromptInput && ModelPicker"),
        ),
        KeyBinding::new("ctrl-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("super-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("ctrl-v", Paste, Some("PromptInput")),
        KeyBinding::new("cmd-v", Paste, Some("PromptInput")),
        KeyBinding::new("super-v", Paste, Some("PromptInput")),
        KeyBinding::new("alt-v", Paste, Some("PromptInput")),
        KeyBinding::new("cmd-shift-v", PastePlainText, Some("PromptInput")),
        KeyBinding::new("cmd-alt-shift-v", PastePlainText, Some("PromptInput")),
        KeyBinding::new("ctrl-c", Copy, Some("PromptInput")),
        KeyBinding::new("cmd-c", Copy, Some("PromptInput")),
        KeyBinding::new("super-c", Copy, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-c", Copy, Some("PromptInput")),
        KeyBinding::new("ctrl-x", Cut, Some("PromptInput")),
        KeyBinding::new("cmd-x", Cut, Some("PromptInput")),
        KeyBinding::new("ctrl-z", Undo, Some("PromptInput")),
        KeyBinding::new("cmd-z", Undo, Some("PromptInput")),
        KeyBinding::new("super-z", Undo, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-z", Redo, Some("PromptInput")),
        KeyBinding::new("cmd-shift-z", Redo, Some("PromptInput")),
        KeyBinding::new("ctrl-y", Redo, Some("PromptInput")),
        KeyBinding::new("ctrl-w", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("alt-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("alt-delete", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("cmd-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("ctrl-delete", DeleteWordForward, Some("PromptInput")),
        KeyBinding::new("alt-d", DeleteWordForward, Some("PromptInput")),
        KeyBinding::new("ctrl-u", KillToStart, Some("PromptInput")),
        KeyBinding::new("ctrl-e", End, Some("PromptInput")),
        KeyBinding::new("ctrl-k", HistoryPrev, Some("PromptInput")),
        KeyBinding::new("ctrl-j", HistoryNext, Some("PromptInput")),
        KeyBinding::new("ctrl-[", HistoryPrev, Some("PromptInput")),
        KeyBinding::new("ctrl-]", HistoryNext, Some("PromptInput")),
        KeyBinding::new(
            search_key(ChoiceKey::Prev),
            HistoryPrev,
            Some("PromptInput"),
        ),
        KeyBinding::new(
            search_key(ChoiceKey::Next),
            HistoryNext,
            Some("PromptInput"),
        ),
        KeyBinding::new("alt-shift-up", GrowEditor, Some("PromptInput")),
        KeyBinding::new("alt-shift-down", ShrinkEditor, Some("PromptInput")),
        KeyBinding::new(search_key(ChoiceKey::Dismiss), Clear, Some("PromptInput")),
        KeyBinding::new("ctrl-s", Stash, Some("PromptInput")),
        // Falls through (propagates) unless ghost text is showing.
        KeyBinding::new(
            search_key(ChoiceKey::Complete),
            AcceptCompletion,
            Some("PromptInput"),
        ),
        KeyBinding::new("home", Home, Some("PromptInput")),
        KeyBinding::new("end", End, Some("PromptInput")),
        KeyBinding::new("ctrl-home", Home, Some("PromptInput")),
        KeyBinding::new("ctrl-end", End, Some("PromptInput")),
        KeyBinding::new(search_key(ChoiceKey::Confirm), Submit, Some("PromptInput")),
        KeyBinding::new("shift-enter", InsertNewline, Some("PromptInput")),
        // Outranks the ChatPanel shift-space jump. With a draft it types a
        // space (Shift is often still held mid-sentence), otherwise it
        // propagates so an empty composer still jumps to latest.
        KeyBinding::new("shift-space", ShiftSpace, Some("PromptInput")),
        KeyBinding::new("ctrl-enter", Queue, Some("PromptInput")),
    ]);
    // Registered after the composer keys so the effort chords outrank word
    // motion in a chat composer. See `effort::bind_keys` for the fallthrough.
    crate::effort::bind_keys(cx);
}
