# Desktop tab recovery

Launching Jcode Desktop after an unexpected exit automatically restores the last
workspace checkpoint when its owner is no longer running and the checkpoint is
at most 30 seconds old. A normal application quit saves the final checkpoint
and restores it on the next launch without the age limit. Launching while the
app is already running continues to show its existing window.

The UI checkpoints once per second. This bounds the usual unsaved layout/draft
window to about one second. The 30-second cutoff uses the last successful
checkpoint, not an OS-provided crash timestamp. A blocked UI or failed disk write
can make the checkpoint older, in which case recovery conservatively starts fresh.

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

## Recover tabs after a delayed restart

After the 30-second window the app starts fresh instead of automatically
reopening the crashed workspace. Before replacing an expired crash checkpoint
with tabs, Desktop retains it next to the current checkpoint as
`crash-recovery.missed.json` (or the matching mode-specific `.missed.json` file).
The archive contains drafts and is owner-only on Unix. It is not loaded at
startup. Each mode retains the most recently archived checkpoint.

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
cargo test -p jcode-desktop-ui --lib workspace::recovery -- --test-threads=1
cargo build -p jcode-desktop -p jcode-desktop-ui
python3 scripts/accept-crash-recovery.py target/crash-accept-main
python3 scripts/accept-crash-recovery.py target/crash-accept-workspace --no-sidebar
python3 scripts/accept-crash-recovery.py target/crash-accept-reload --reloads 1
```

The acceptance script uses offline fixture sessions, an isolated home and runtime
directory, and private Xvfb/Openbox, never the user's running app or sessions.
It verifies SIGKILL restoration, native draft editing, focus and widths, archival
of expired checkpoints, and restoration after a normal quit. Screenshots must
contain nonblank app pixels. Each run pins private copies of the executable and optional UI plugin
to avoid interference from concurrent builds. `--reloads` exercises native Ctrl+R
activation with the prebuilt plugin and a no-build Cargo shim, not compilation.
