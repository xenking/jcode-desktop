# Transcript tools and change review

Users disclose tool output inline and open file changes in a focused review
surface without losing their conversation.

## Sub-features

- transcript.tools: expand/collapse one tool's output without changing others.
- transcript.rows: capture an actually visible row, not just an offscreen item.
- transcript.review: Review metadata opens the correct file diff.
- transcript.return: return from review preserves chat and draft.

## How to get to it (user POV)

Click a tool's token-count button to disclose its output. Other row/card areas
are not toggles. For edits, click the file path/count metadata or Review, select
files in the review tree, then Escape or Back to chat/Close review to return.

## Driving it with native-control

Preconditions: new healthy helper run with default `expandable` fixture, at
least one visible tool and no earlier `tools-*` artifacts.

- **Toggle:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --journey tools`.
  It reads `tool:<call_id>` from laid-out visible rows, waits for visibility,
  captures before, clicks the production semantic toggle, requires membership in
  `expanded_regions` to change only for that tool and the draft to survive, then captures after. Require
  `tools-proof.json: passed=true` and inspect both PNGs for disclosed/collapsed
  output, not just changed hashes.
- **Identity:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action state`.
  Compare exact session and tool IDs with `tools-before.json`. Other transcript
  content must remain intact. `actions.jsonl` records the exact click.
- **Native token-only hit testing, Linux:**
  `python3 scripts/accept-inline-tools.py target/tool-hit-proof.png`.
  Requires current Linux host at `target/debug/jcode-desktop` and Pillow. The
  existing private-Xvfb runner checks button, row and card-body input, expanded
  pixels and collapse restoration.
- **Native review paths, Linux:**
  `python3 scripts/accept-diff.py target/diff-proof.png`.
  Also needs Tesseract. It clicks both Review entries, selects a file, verifies
  the chat remains, and closes by Escape/Close.

## Gotchas

- `all` can scroll tools offscreen in a standalone window. The first real run
  failed this precondition, retained evidence and cleaned up. Use the existing
  compact `expandable` seed, not offscreen setters or relaxed assertions.
- Semantic tool click invokes the production toggle, not the token-button
  pointer hitbox. Noninteractive row/card-body behavior needs native input.
- A fixture PNG alone is not a click proof. Retain state plus the action trace.
- Row selectors contain revisions and need rereading after transcript changes.
  Capture fails if a row is not uniquely visible in the last frame.
- Diffs are tool-input snapshots, not repository status. macOS has no Review/tree
  selector here. That entry point remains a gap.
