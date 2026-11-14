# Voice backends

Settings > "Voice backend · all Jcode clients" edits the shared jcode config
(`$JCODE_HOME/config.toml`, `[dictation]`), so the TUI, CLI and Desktop agree.

| Choice | Writes | Recording path |
| --- | --- | --- |
| Auto | `backend = "auto"` | Nari key if present, else Soniox key |
| Nari | `backend = "nari"` | `NariRecording::start_configured`, Nari realtime |
| Soniox | `backend = "soniox"` | same handle, Soniox realtime (`run_nari_events`) |
| Jcode subscription | `backend = "subscription"` | not streamed yet in Desktop, shows an explanatory error. Endpoint is `JCODE_API_BASE` / `jcode-subscription.env` |
| External command | `backend = "command"`, `command = "..."` | runs the command, its stdout is the transcript |

"Custom endpoint" sets `[dictation] base_url` (a `wss://` override, or `ws://`
to a loopback IP for a local proxy). "Set API key" uses a masked input and
`StreamingBackend::save_api_key`, which writes `NARI_API_KEY` to
`~/.config/jcode/nari.env` or `SONIOX_API_KEY` to `~/.config/jcode/soniox.env`
with owner-only permissions. Those are the same places jcode-voice reads.

## SuperWhisper

Orca's `superwhisper-importer.ts` extracts SuperWhisper's license and request
signing key from one specific app build (byte offsets in the binary plus the
keychain) and then calls `api.superwhisper.com` directly. We did not port that:
it breaks on every SuperWhisper update, needs a keychain prompt, and uses a
private API.

Instead, Desktop uses a running SuperWhisper as an external transcriber.
"Use SuperWhisper" (macOS, shown only when `/Applications/superwhisper.app`
exists) installs `~/.jcode/bin/superwhisper-dictate.sh` and selects it as the
command backend. The script opens `superwhisper://record`, waits for the next
folder in `~/Documents/superwhisper/recordings/`, and prints `meta.json`'s
`result`. SuperWhisper keeps ownership of the microphone, model, mode and
license. Stop the recording with SuperWhisper's own shortcut. Pressing the
Jcode voice button again cancels the wait. `SUPERWHISPER_RECORDINGS` and
`SUPERWHISPER_TIMEOUT` (seconds, default 300) override the defaults.
