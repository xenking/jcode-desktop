# Desktop idle work

Audit scope: timers, channel waits and periodic refreshes in the Desktop host,
workspace, panels and Desktop harness. See `workspace_housekeeping`,
`workspace_restart`, `workspace_recovery` and `unix_listener_ready` for the
cadences that apply to an otherwise idle window.

| Work | Idle behavior |
| --- | --- |
| Restart manifest | Compare persisted content before replacing the file. Ignore follow-tail pixel offsets and the old process's PTY cursors. Debounce edits with a bounded deadline. Inactive, settled windows sample every 10 seconds, and activation wakes them immediately. |
| Crash checkpoint | Save changes on the next one-second capture. An unchanged workspace writes only a 10-second heartbeat, which is still shorter than the 30-second recent-crash window. |
| Session catalog and edit-stat sidecars | Initial load and explicit refresh still run. A foreground workspace reconciles at the configured interval, and refocus catches up. A background workspace does not poll the catalog, edit-stat sidecars, or disk streaming markers. |
| OpenRig HUD | One process-wide feed. No fetch when no Desktop window is active. Focus wakes it, and only changed snapshots notify the UI. |
| Desktop and preview control sockets | Workers wait for socket readiness or shutdown. No 10-millisecond accept retries when no client is connecting. |
| Draft models | A startup request, not a repeating poll. Live model events arrive via an attached session, and explicit model actions remain available. |
| Account status and usage | One process-wide minute refresh shared across windows, plus event-driven login refresh. It also keeps background-session quotas current. No per-window backend loop. |
| Background task watch | Desktop registers once per capable attachment and again when the selected task output changes. Repeated task reads are a bridge responsibility, not a Desktop timer. |

Remaining conditional wakes have narrower owners: open terminal panels drain the
PTY over the host ABI, the Cloud monitor runs after using Machines or a Cloud
connection, OpenRig and orchestration panels refresh their own open surfaces,
and native Applet pumps service provider output and effects. The resume picker
checks presence only while open. Animation/reveal, voice metering, endpoint
handoff and reconnect timers run for active work or bounded retries. Host CPU
monitoring and opt-in profiling are diagnostics. The packaged updater keeps its
four-hour check. These cadences are not part of the session catalog loop.

## Native restart-write verification

Use `scripts/verify-jcode-desktop.py launch --run <new-dir> --fixture expandable
--persist-restart-manifest` on macOS. The option only enables writes under the
owned profile's `restart/windows` directory. Compare file modification times
after the first save during a 60-second idle window, then send one `set_composer`
action through the private control socket and wait for the new draft to appear in
the manifest. Run `cleanup` with the same `--run` afterward. This offline fixture
does not measure production daemon connections or provider usage traffic.

On November 22, 2026, the original live audit recorded about 40 restart
manifest rewrites (1.2 MB) per minute. A subsequent live read-only sample
counted 19 rewrites (618,139 bytes), including genuine draft changes. The
patched hidden macOS fixture recorded **0 rewrites and 0 bytes** in 60 seconds.
One private composer edit produced one manifest replacement in 7.12 seconds,
with no more writes during the next 12 seconds. The real-file regression also
permits manual scrolling and ignores 60 consecutive follow-tail offset updates.
The simulated cadence gates changed hidden catalog refreshes from 12 to 0 per
minute at a five-second setting, and unchanged crash checkpoints from 60 to 6.
