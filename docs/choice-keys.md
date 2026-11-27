# Keys for answering a list

Ask cards, MCP approval cards and the composer pickers share one key map
(`crates/jcode-desktop-ui-core/src/choice_keys.rs`). An MCP approval is an ask
card with four options, so it follows the ask card column. A key means the same
thing everywhere it applies; the pickers leave out the keys a search field
needs for typing.

| Key | Ask or approval card, rows focused | Free-text field of the card | Picker (search field) |
| --- | --- | --- | --- |
| Up, Down | Move the highlight; at a tab's first or last row cross into the neighbouring tab; stop at the card's ends | Moves the caret | Move the highlight, stopping at the ends |
| 1 to 9 | Pick option 1 to 9 (toggles in a multi-select) | Typed | Typed |
| Space | Toggle the highlighted option of a multi-select | Typed | Typed |
| Enter | Pick the highlighted option and move on (a multi-select with nothing chosen takes it; one with answers keeps them); on the own-answer row open its field; on the review reopen the question or send the answers | Move on | Confirm the highlighted row |
| Esc | Hand the keys to the composer; the card stays pending (click it to take them back) | Back to the rows | Close the picker |
| Tab, Right | Next question; Tab on the last tab hands the keys to the composer | Tab: next question | Tab: complete the highlighted row |
| Shift+Tab, Left | Previous question | Shift+Tab: back to the rows | Typed |
| `n` | Add a comment to the highlighted option | Typed | Typed |

- Rows are a question's options, then its own-answer row; the review's rows are
  one per question, then the submit stop. Nothing wraps: the card stops at its first and last row.
- Only the first nine options have numbers.
- The highlight starts on the option labelled `(Recommended)`; nothing is
  selected until the user picks or presses Enter on it.
- Focus stays on the card after an answer: the next queued question opens with
  the keyboard still on it, and the composer takes over when the last one closes.
- Control, Alt and Command chords are never part of this map.
- Discard is the close button on the card; no key declines an ask.

## Card layout

- The card is as tall as its content, up to 45% of the window, and scrolls
  beyond that. Dragging the handle on its top edge sets a cap (up to 80% of the
  window); a cap taller than the content leaves no blank area. Double-click the
  handle to return to the automatic cap. The cap is remembered for the session.
- An option is a label (number, `Recommended` badge, comment button) with its
  whole description under it. There is no expand control.
- The question renders with the chat's markdown renderer. An applet mounted
  inline on the ask call appears in the card through the applet surface.
- Text in the card is the chat's body size (`markdown::CHAT_TEXT_SIZE`); there is no key hint line.
