# Nested swarm sidebar

Root sessions remain ordinary sidebar entries. Sessions with swarm ownership metadata are rendered inside their root's entry rather than duplicated as top-level history entries. Ordinary transcript forks do not imply swarm ownership.

A coordinator's workers are compact one-line rows nested under it: indentation, a status glyph, the worker's name, its task label and its age. Rows are 18px, 10.5px text and never carry a decorative rail, so a coordinator with four workers stays one scannable block. Clicking a row opens or focuses that worker's conversation; the coordinator's own entry stays where it was.

## Status glyphs

Each worker row shows one SVG glyph for the latest persisted state, in the same vocabulary the CLI uses:

| Glyph | State |
| --- | --- |
| spinner (spins, 900ms) | running |
| check | ready / done |
| cross | failed |
| pause | blocked / waiting |

The spinner is its own leaf view ticking at 20Hz on the shared decorative grid, so a live worker stays visibly live without rebuilding the sidebar, header or rows around it. Non-running glyphs are plain still icons. `reduce_motion` (system or appearance setting) stops both the spin and the breathing pulses.

## Folding

Long-running coordinators own dozens of workers. Every live worker is always shown, plus the latest three finished ones; the rest fold into a dim `+N finished` line under the block. Folding never hides a live worker, and the fold count is part of the row's layout key, so a worker finishing re-measures the row rather than re-creating the tree.

## Composer bar

The old text strip above the composer ("Waiting for N workers", the last report line, worker chips) is gone. A coordinator now shows a pill in the composer's meta row: the live-worker glyph plus the count, only while at least one worker is live. Clicking it lists up to twelve workers with their status; each row opens that worker's chat. The same row also holds the agent activity icon:

| Icon | Meaning |
| --- | --- |
| idle dot | no turn running, no live worker |
| thinking | a turn is running |
| tool | the running turn is executing a tool |
| hourglass | this agent is idle but owns a live worker |

Both are leaf views, so their animation never rebuilds the composer, and both carry a tooltip naming the state.

## Transcript cards

An inline `swarm` tool call (spawn, await_members, status, message, stop, resume) renders as worker rows, not as a tool-call size: animal icon, status glyph, task label, live status, the worker's own model and provider, and that worker's total token spend read from its session record. The coordinator's call output size is never shown as a worker figure. Clicking a row opens that worker's chat. A call that names no worker the UI knows about claims no status and shows no usage rather than guessing.

## Data

The SDK session API exposes optional `parent_session_id`, `agent_label`, and `swarm_status` fields. Ownership is sourced from durable swarm `report_back_to_session_id`, not the session transcript's fork parent. Local desktop refreshes use the shared SDK enrichment helper for compatibility with already-running older daemons. Remote parent IDs are namespaced to the same host as the child. Status describes the latest persisted swarm snapshot, not an independent liveness probe. Worker model and spend come from the worker's own persisted session record (`crates/jcode-desktop-harness/src/swarm_usage.rs`), off the UI thread.

## Verification

Current regression commands:

```sh
cargo test -p jcode-desktop-ui --lib swarm
python3 scripts/screenshot.py target/swarm-review.png --swarm
python3 scripts/screenshot.py target/swarm-native.png --swarm-interact --no-build
```

Use `--no-build` only after a current paired Desktop build. Native acceptance drives the real UI on private Xvfb: OCR checks the three worker rows are nested and indented under their coordinator with no text strip or expansion controls, and native clicks check that a worker row opens that worker's chat while the lead row keeps its own panel. Per-step PNGs and a `.swarm.json` report accompany the capture.

`JCODE_DESKTOP_SCREENSHOT_SWARM=real` renders the user's own coordinator and workers from trimmed copies of real `~/.jcode` session files (`crates/jcode-desktop-harness/src/fixtures/microbe_home` plus `usage_home`), so a review capture shows real data shapes instead of invented ones. `JCODE_DESKTOP_SCREENSHOT_SWARM=1` still renders the small three-agent fixture the native acceptance uses.

GPUI tests cover exact nesting geometry, all four status glyphs, the fold count, the composer pill's presence and absence, the activity icon's four states, and every entry point that opens a worker (sidebar row, composer list row, transcript card).

The session catalog remains bounded. A child whose parent is outside the loaded catalog stays visible independently until its parent is available.
