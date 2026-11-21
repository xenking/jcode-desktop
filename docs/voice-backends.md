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

In the chat composer, Nari and Soniox show live text at the caret in `insert`
mode. Stop voice (square button or Ctrl+Shift+V) finishes the recording and
keeps the result in the draft. Other delivery modes keep their existing behavior.

## SuperWhisper

Orca's `superwhisper-importer.ts` extracts SuperWhisper's license and request
signing key from one specific app build (byte offsets in the binary plus the
keychain) and then calls `api.superwhisper.com` directly. We did not port that:
it breaks on every SuperWhisper update, needs a keychain prompt, and uses a
private API.

Instead, Desktop uses a running SuperWhisper as an external transcriber.
"Use SuperWhisper" (macOS, shown only when `/Applications/superwhisper.app`
exists) installs `~/.jcode/bin/superwhisper-dictate.sh` and selects it as the
command backend. The script opens `superwhisper://record/start`, waits for the next
folder in `~/Documents/superwhisper/recordings/`, and prints `meta.json`'s
`result`. SuperWhisper keeps ownership of the microphone, model, mode and
license. The square Stop voice button and Ctrl+Shift+V finish capture with
`superwhisper://record/stop` (SuperWhisper 2.16.3 or newer). The bridge receives
only the finished transcript, including when SuperWhisper uses a realtime model.
`SUPERWHISPER_RECORDINGS` and `SUPERWHISPER_TIMEOUT` (seconds, default 300)
override the defaults. Select **Use SuperWhisper** again to update an existing bridge.
