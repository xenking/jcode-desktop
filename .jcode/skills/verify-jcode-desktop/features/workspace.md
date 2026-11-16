# Workspace and panel navigation

Users arrange coding sessions across four workspaces, switch focus without
changing conversations, and preserve drafts and panel order.

## Sub-features

- workspace.tabs: session-tab click focuses the intended composer.
- workspace.horizontal: previous/next moves one position, including edges.
- workspace.vertical: workspace switching restores remembered focus.
- workspace.overview: Overview owns focus, not hidden composers.
- workspace.lifecycle: opening/closing preserves neighboring panels.

## How to get to it (user POV)

Use a normal workspace window, session tabs, numbered workspace controls,
Cmd/Super+H/L for panel focus, Cmd/Super+J/K for workspace movement, and + for new
sessions. Overview is a separate surface. The macOS helper intentionally opens
single-panel mode, without workspace chrome.

## Driving it with private-Xvfb acceptance

Preconditions: Linux, current Linux host from
`justrust build -p jcode-desktop --target-dir target`, a Jcode executable, Xvfb,
Openbox, xdotool, ImageMagick and Mesa lavapipe. Use short fresh output paths.

- **Focus/edges/workspaces/Overview:**
  `python3 scripts/accept-navigation.py target/v-nav --binary target/debug/jcode-desktop --reloads 0 --linked-ui`.
  Require exit 0 and `navigation.jsonl` checkpoints matching row, panel order,
  focused slot and keyboard owner. Inspect `navigation.png`. The runner starts
  private real SDK sessions without inference.
- **Dense tabs:**
  `python3 scripts/accept-navigation.py target/v-tabs --binary target/debug/jcode-desktop --reloads 0 --linked-ui --compact-tabs`.
  Require every native tab click focuses its corresponding session.
- **Native closing:**
  `python3 scripts/screenshot.py target/close-proof.png --binary target/debug/jcode-desktop --no-build --panels 6 --close-interact`.
  Require expected next composer and unchanged remaining panel order.

## Gotchas

- macOS helper keys are only Enter/Escape. It refuses arbitrary navigation input
  rather than injecting globally into the user's desktop.
- Initial macOS skill proof did not run Linux navigation. A tool PNG cannot prove
  workspace keyboard, tab hit-testing or reload correctness.
- `--reloads 0 --linked-ui` avoids compiling inside acceptance. A reload change
  requires its own matched-host/UI acceptance, not this navigation-only proof.
- Long private Unix-socket paths fail deliberately. See
  `docs/navigation-verification.md` and `docs/single-panel.md` for contracts.
