# Desktop tab recovery

Launching Jcode Desktop after an unexpected exit automatically restores the last
workspace checkpoint for the launch mode once its owner is no longer running.
There is no expiration. The old checkpoint stays on disk until the replacement
workspace has applied it. If the new launch requests a session, Desktop opens or
selects it after restoring the tabs. A normal quit still saves the final state
for the next ordinary launch. Launching while the app is already running
continues to open or show a window as before.

The UI samples once per second and saves workspace changes such as tab layout or
draft edits. An unchanged workspace writes a heartbeat every ten seconds. The
session picker's query, selection and keyboard focus do not trigger a crash
checkpoint write. Crash recovery returns to the active tab unless the launch
explicitly requests the picker or another session.

Restoration preserves session panels, order, rows, widths, active panel, drafts,
and transcript scroll position through the existing workspace snapshot format.
Session histories reconnect to the runtime. Prompts are not resubmitted. Terminal
panels start new PTYs in their saved directories because host resource IDs cannot
survive a process exit.

Checkpoints live beside the learning state:

- `$XDG_STATE_HOME/jcode-desktop/crash-recovery.json`, when configured.
- `~/.local/state/jcode-desktop/crash-recovery.json` on Linux by default.
- `~/Library/Application Support/Jcode/crash-recovery.json` on macOS by default.

`--no-sidebar` and `--workspace` share `crash-recovery-no-sidebar.json`.
The primary `--single-panel` window uses `crash-recovery-single-panel.json`.
Files contain draft content and use owner-only permissions on Unix. Checkpoints
are atomically replaced under a file lock. A generation token prevents a retired
hot-reload worker or its quit callback from overwriting a newer generation.
Recovery is conservatively disabled for foreign processes on non-Unix platforms,
where process liveness detection has not yet been implemented.

## Inspect a checkpoint manually

Older versions used a 30-second cutoff and copied expired checkpoints to
`crash-recovery.missed.json` (or a mode-specific `.missed.json` file). These old
archives are not loaded on startup. They can still be inspected manually.

List tab positions, the selected tab, exact session IDs, and titles without
launching Desktop or changing its live state. Use `jq` on a private copy:

```sh
jq -r '.snapshot as $s | $s.slots | to_entries[] |
  [.key, (if .key == $s.active then "selected" else "" end),
   .value.panel.session_id, .value.panel.title] | @tsv' /path/to/private-copy.json
```

The same command works on a saved `restart/windows` manifest. In Desktop,
search for each wanted session ID with Resume. Leave the selected tab closed
if it may have triggered the crash. This reopens session history, not the
archived layout or local unsent drafts.

## Hot reload

Ctrl+R installs checkpointing into an already-running development host without a
restart. Existing in-memory hot-reload snapshots take precedence over disk
recovery. A launch cannot recover a crash that happened before a checkpoint-capable
UI was installed.

## Verification

```sh
justrust test -p jcode-desktop-ui --lib workspace::recovery
justrust build -p jcode-desktop -p jcode-desktop-ui
python3 scripts/accept-crash-recovery.py target/crash-accept-main
python3 scripts/accept-crash-recovery.py target/crash-accept-workspace --no-sidebar
python3 scripts/accept-crash-recovery.py target/crash-accept-reload --reloads 1
```

The acceptance script uses offline fixture sessions, an isolated home and runtime
directory, and private Xvfb/Openbox, never the user's running app or sessions.
It verifies SIGKILL restoration, native draft editing, focus and widths, a
delayed relaunch with a session argument, and restoration after a normal quit. Screenshots must
contain nonblank app pixels. Each run pins private copies of the executable and optional UI plugin
to avoid interference from concurrent builds. `--reloads` exercises native Ctrl+R
activation with the prebuilt plugin and a no-build Cargo shim, not compilation.
