---
name: verify-jcode-desktop
description: "Verify Jcode Desktop's real native Rust/GPUI UI after behavior or visual changes. Use for agent acceptance evidence, Desktop regression reproduction, or proving a PR without touching the user's app."
---

# Verify Jcode Desktop

Read [features/README.md](features/README.md), then the affected recipes. Work from
the repo root. This skill verifies a private instance, not the user's daily driver.

## Launch

Requires macOS with a graphical login session, Xcode, Rust, Python 3.9+, `justrust`,
and the sibling `../jcode` checkout. No provider credentials are needed.

```sh
RUN=target/verify-desktop-proof
python3 scripts/verify-jcode-desktop.py launch --run "$RUN"
```

The evidence directory must be new. Launch always builds the current checkout:
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust build -p jcode-desktop --target-dir "$PWD/target/verify-build"`.
It resolves Xcode's SDK explicitly, discarding the CommandLineTools `SDKROOT`
that Apple's system Python can inject. A dedicated build target prevents another
agent replacing this run's executable. Build failures retain `build.log` and start
no app. Build cache remains after cleanup.

Readiness means an owned native host, private endpoint, exact window/session IDs
and laid-out fixture transcript rows, not just an open socket. `ready.json` and a
healthy doctor establish this. An empty fixture requires an addressable composer.
The default is the existing compact `expandable` transcript; `all` can put every
tool offscreen in a standalone window and must not be treated as tool readiness.

The helper imports `screenshot.isolated_env` and the actual
`accept-native-control.py:request` transport. It runs the production host/UI as a
**hidden, non-activating native window**, using `JCODE_DESKTOP_SCREENSHOT=1` plus
`JCODE_DESKTOP_CONTROL_TEST=1`. Existing fixtures replace external conversation
and session data, not the GPUI renderer or production interaction handlers.
No daemon, bridge, provider, model request or global input injection is used.

A supervisor owns the Desktop `Popen` handle. Its 0700 temporary root has separate
HOME, JCODE_HOME, XDG data/config/cache/state/runtime, instance socket and control
directory. Private daemon/API sockets deliberately do not exist. User settings,
desktop sockets, SSH and provider environment are not inherited by the app.
`isolation.json` records the exact environment. The reviewed startup gate disables
window exposure, activation and global shortcut registration. The supervisor
expires after 900 seconds. Use `launch --lifetime 1800` for a longer run, max 3600.

**Fail closed:** the helper refuses non-macOS launches and checkouts lacking the
reviewed hidden/non-activating gate. If isolation or doctor fails, clean up and
report why. Never fall back to an existing window, global `desktop list`,
`desktop_selfdev reload`, the shared harness bridge, process-name killing or
`--new-process` as a profile guarantee. The user's app/bridge stay untouched.

Linux recipes in the feature map reuse existing private-Xvfb runners, with
Linux-built binaries, Xvfb, Openbox, xdotool, ImageMagick, Mesa lavapipe and
journey-specific dependencies. They own launch/teardown internally. They are not
substitutes for native macOS input acceptance. Windows/FreeBSD remain unmapped.

## Doctor

```sh
python3 scripts/verify-jcode-desktop.py doctor --run "$RUN"
```

Require exit 0 and `healthy: true`. The read-only application check verifies the
supervisor's child is alive, private endpoint is PID-scoped, exact window/session
match, binary SHA-256 is unchanged, and Desktop plus sibling Jcode build inputs
(including dirty Rust/TOML/lockfiles) still match the completed build. It saves
`doctor.json`. Auth is **not exercised**, not silently assumed valid. Changed
inputs require cleanup and a fresh launch.

## Drive

Start with a mapped deterministic journey:

```sh
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --journey tools
# Alternatives on a fresh run: --journey composer or --journey search
```

Journeys first seed a fixed private draft so rotating composer suggestions cannot
fake a screenshot difference. They save before/after native PNGs and semantic states, perform a production
UI action, assert its postcondition and image change, then write `tools-proof.json`.
Existing journey artifacts are never replaced. Missing fixture data is a failed
journey, never permission to weaken its assertions.

For targeted interaction, use the same semantic protocol as Jcode's `desktop`
tool, routed exclusively through this run's supervisor:

```sh
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action list
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action state
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action set_composer --params '{"text":"Verification draft"}'
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action click --params '{"selector":"composer.send"}'
```

Actions: `list/open_session/state/click/set_composer/send/press_key/search/expand_tool/wait_for`.
Parameters follow `scripts/accept-native-control.py` and
`crates/jcode-desktop-ui/src/workspace/controls/workspace_control.rs`. Read
`visible_transcript_items[].selector` before tool/row actions. Tool clicks use
`tool:<call_id>`. `expand_tool` takes the call ID **without** `tool:`. Ask selectors
come from `panel.ask.option_selectors`. Row selectors include a revision and must
be reread after transcript changes. `wait_for` accepts 1..5000 ms here, matching
the imported transport timeout. `press_key` supports only Enter and Escape.

For swarm navigation, `click` accepts `sidebar-worker:<session_id>`,
`swarm-status` (the coordinator's composer pill), and `status-worker:<session_id>`
(a worker in the open list). Target the coordinator for the pill and its list.
These selectors dispatch pointer events in the private GPUI window and reject
missing or ambiguous targets. They are disabled in the user's app. See
[the swarm worktree journeys](features/swarm.md).

The helper supplies target IDs and permits mutations only in its own hidden
window. `--session ID` targets another already-open private session.
`open_session` forces `background_panel` in the owned window, never `new_window`.
The private catalog must contain the session. Caller-supplied targets, destinations,
permissions and output paths are rejected. No route discovers the user's windows.

## Evidence

```sh
python3 scripts/verify-jcode-desktop.py screenshot --run "$RUN" --name final.png
# Crop a uniquely visible tool or revision-scoped row, with a fresh output name:
# For the default expandable fixture, after checking state:
# python3 scripts/verify-jcode-desktop.py screenshot --run "$RUN" --name tool.png --selector tool:expandable-title
```

PNG evidence is the real window's Metal `render_to_image`, not an OS screenshot of
the user's desktop or a reconstructed preview. Inspect the PNGs with your image
reader. A changed hash alone is not visual correctness.

Retain `build.log`, `run.json` (source/build identities), `ready.json`,
`isolation.json`, `doctor.json`, `actions.jsonl` (ordered requests and responses),
before/after state and PNGs, proof reports, app/supervisor logs and cleanup/exit
receipts. Keep `run.json`'s local supervisor capability private while it is alive.

Proof standards:
- Capture action and resulting state for every claimed user entry point.
- Check side effects independently. Tool expansion needs changed
  `expanded_regions` **and** visibly disclosed output. A live prompt needs
  persisted history/provider response, not just an emptied fixture composer.
- Semantic control exercises production handlers, but **does not prove OS mouse
  hit testing, keyboard dispatch, IME, clipboard, focus activation or storage**.
  Use the existing private-native-input acceptance runners for those claims.
- Offline fixtures omit backend writes, authentication and inference. Check
  `isolation.json`, action paths and app logs for unexpected connections. This is
  not a network sandbox or packet-level isolation proof.
- Missing seeds, unsupported platforms and untested entry points stay gaps.

## Cleanup

Run cleanup on success **and after every failed drive**, before the next attempt:

```sh
python3 scripts/verify-jcode-desktop.py cleanup --run "$RUN"
python3 scripts/verify-jcode-desktop.py cleanup --run "$RUN"  # idempotent
# Evidence survives:
test -s "$RUN/tools-after.png" && test -s "$RUN/actions.jsonl" && test -s "$RUN/cleanup.json"
```

The supervisor terminates/waits only on its retained child handle, escalating to
kill only for that child. A PID in a file is never kill authority. Cleanup removes
only the temporary profile/sockets after checking its private ownership marker,
and writes `process-exit.json`/`cleanup.json`. Runtime logs are copied into evidence.
Evidence and build cache survive. Normal supervisor termination, startup failure
and lifetime expiry also clean up. If the supervisor is forcibly killed or its
capability is lost, cleanup refuses rather than signaling an unverified PID.
Report that exception. Do not touch the shared app/bridge to recover.

## Helpers

Executable: `scripts/verify-jcode-desktop.py`. Discover flags with subcommand
`--help`. Safety tests:

```sh
python3 -m unittest discover -s scripts -p 'test_verify_jcode_desktop.py' -v
```

See [the feature map](features/README.md) for other user paths and Linux harness reuse.
Use `/maintain-verification-skill` as features or harness contracts change.
