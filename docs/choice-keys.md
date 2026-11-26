# Keys for answering a list

Ask cards, MCP approval cards and the composer pickers share one key map
(`crates/jcode-desktop-ui-core/src/choice_keys.rs`). An MCP approval is an ask
card with four options, so it follows the ask card column. A key means the same
thing everywhere it applies; the pickers leave out the keys a search field
needs for typing.

| Key | Ask or approval card, options focused | Free-text field of the card | Picker (search field) |
| --- | --- | --- | --- |
| Up, Down (`k`, `j`) | Move the highlight, wrapping at the ends | Moves the caret | Move the highlight, stopping at the ends |
| Home, End | First, last option | Moves the caret | Moves the caret |
| 1 to 9 | Pick option 1 to 9 (toggles in a multi-select) | Typed | Typed |
| Space | Toggle the highlighted option of a multi-select | Typed | Typed |
| Enter | Confirm: next question, or submit on the last | Confirm | Confirm the highlighted row |
| Esc | Dismiss: asks to decline; Enter confirms, Esc keeps answering | Back to the options | Close the picker |
| Tab | Go to the free-text field | Walks on to the card's buttons | Complete the highlighted row |
| Shift+Tab | Walks back through the card's buttons | Back to the options | Typed |
| Left, Right | Previous, next question | Moves the caret | Moves the caret |
| `n` | Add a comment to the highlighted option | Typed | Typed |
| `e` | Expand or collapse the highlighted option's description | Typed | Typed |

- A card wraps from the last option to the first; a picker stops at its ends so a stray arrow never throws a typed query to the far end of the list.
- Only the first nine options have numbers. A longer list is walked with the
  arrows, Home and End, and the highlight scrolls into view.
- The highlight starts on the option labelled `(Recommended)`; nothing is
  selected until the user picks. Enter on a question with nothing picked skips it.
- Focus stays on the card after an answer: the next queued question opens with
  the keyboard still on it, and the composer takes over when the last one closes.
- Control, Alt and Command chords are never part of this map.

## Card layout

- The card is at most 45% of the window height by default and scrolls
  inside; drag the handle on its top edge to resize it, and double-click the
  handle to return to automatic height. The height is remembered for the
  session, across questions.
- An option is one line: number, label, a `Recommended` badge and the first line
  of its description, cut with an ellipsis. Longer descriptions get an expand
  toggle that shows them as markdown. The expand toggle appears for descriptions
  of more than 48 characters or more than one line, a fixed estimate of what one
  line holds, not a measurement.
- Question and expanded descriptions render with the chat's markdown renderer:
  headings, lists, code blocks with highlighting, tables and links. An applet
  mounted inline on the ask call appears in the card through the applet surface.
- The tab strip, key hint and answer row never wrap.
