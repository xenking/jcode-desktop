# Jcode Desktop verification map

Read the index and affected recipes before driving. These five primary user
features are the maintained verification source, not five permanently passing
journeys. macOS semantic-control and Linux native-input proof are separate.

## Baseline preconditions

- Repository root, sibling Jcode checkout, Rust and justrust.
- macOS: `RUN=target/verify-desktop-proof`, then
  `python3 scripts/verify-jcode-desktop.py launch --run "$RUN"`. Launch builds
  the app, creates an empty private profile, seeds the existing compact
  `expandable` transcript/session catalog, and waits for usable native UI.
- Require `python3 scripts/verify-jcode-desktop.py doctor --run "$RUN"` to pass.
- Use a fresh run per journey or track cross-feature state explicitly.
- Linux recipes require Linux-built `target/debug/jcode-desktop`. Build with
  `justrust build -p jcode-desktop --target-dir target`. Their runners allocate
  private Xvfb, profiles and sockets and tear down internally. Never target the
  user's display. Dependencies are in each feature file and the runner's help.

## Driving conventions

- Commands are literal, with `$RUN` set above. Read exact selectors from state.
- Keep `actions.jsonl`, before/after states, PNGs, source/build identity and
  cleanup receipts. Inspect images, not only success booleans.
- Each claimed entry point needs its own action/result proof. Enter does not
  automatically cover Send. Semantic control does not cover native OS input.
- Verify storage/network side effects independently when claiming them. Offline
  fixture UI does not prove backend writes, auth, inference or persistence.
- Missing seeds produce a failure, never weaker assertions. Unreachable entry
  points stay gaps. Cleanup must preserve evidence.

## Features

| Feature | User entry points | Verification |
| --- | --- | --- |
| [Chat](chat.md) | Composer, Enter, Send | macOS draft + semantic submission, Linux native first-prompt input |
| [Sessions](sessions.md) | Alt+R, /resume, sidebar history | macOS search, Linux native search/select/resume |
| [Workspace](workspace.md) | Tabs, panel shortcuts, four workspaces | Linux real-daemon native navigation, macOS control gap |
| [Transcript and review](transcript.md) | Tool token button, Review metadata | macOS semantic tool toggle, Linux hit testing/diff review |
| [Terminal](terminal.md) | Cmd/Super+T, shell input | Linux real-shell PTY acceptance, macOS control gap |

## Proof reporting

Actual coverage lives in each run's `*-proof.json`, action log, images and
cleanup receipts. The skill's [proof receipt](../proof.md) records its initial
executed scope. Never count another platform or a different entry point as passed.
