# Voice backends

Settings > "Voice backend · all Jcode clients" edits the shared jcode config
(`$JCODE_HOME/config.toml`, `[dictation]`), so the TUI, CLI and Desktop agree.

| Choice | Writes | Recording path |
| --- | --- | --- |
| Auto | `backend = "auto"` | Nari key if present, else Soniox key |
| Nari | `backend = "nari"` | `NariRecording::start_configured`, Nari realtime |
| Soniox | `backend = "soniox"` | same handle, Soniox realtime (`run_nari_events`) |
| ElevenLabs Scribe | `backend = "elevenlabs"` | same handle, Scribe V2 Realtime with an ElevenLabs API key |
| Jcode subscription | `backend = "subscription"` | not streamed yet in Desktop, shows an explanatory error. Endpoint is `JCODE_API_BASE` / `jcode-subscription.env` |
| External command | `backend = "command"`, `command = "..."` | runs the command, its stdout is the transcript |

Nari and Soniox support a custom endpoint via `[dictation] base_url` (`wss://`,
or `ws://` on loopback). Scribe connects only to the official ElevenLabs endpoint.
**Set API key** saves the selected provider key with owner-only permissions in
`~/.config/jcode/nari.env`, `soniox.env`, or `elevenlabs.env`. You can also set
`NARI_API_KEY`, `SONIOX_API_KEY`, or `ELEVENLABS_API_KEY` in the Jcode process
environment. A SuperWhisper
subscription is not an ElevenLabs API credential. Auto does not select Scribe.

## Which backend and model run

The backend picked here is the one that records, and `[dictation] model` (the
**Model** pill, empty = the provider default such as Soniox `stt-rt-v6`) is the
model it requests. Nothing else is tried:

- A missing key or a failing provider is an error that names the backend and model.
  Only **Auto** chooses (Nari first, then Soniox), and the live label says so.
- `/dictation`, the dictation key and the TUI's `/dictate` follow the same setting.
  A streaming backend records live. The external command runs only for
  **External command** (or **Auto** when a command is set).
- Picking another backend clears the saved model, since a model name belongs to
  one provider.

## Long dictation

There is no recording time limit to configure. A live provider session is renewed
about every minute of audio: the old session is finalized, the next one opens, and
audio spoken meanwhile is queued, so the transcript joins up without gaps. A session
the provider drops on its own is reopened. If the provider cannot be reached again,
the words captured so far are still inserted and a note names the backend and the
reason. The same holds for the external command: when `timeout_secs` is reached it is
interrupted and whatever it already printed is used.

In the chat composer, Nari, Soniox, and Scribe show live text at the caret in `insert`
mode. Stop voice (square button or Ctrl+Shift+V) finishes the recording and
keeps the result in the draft. Other delivery modes keep their existing behavior.

### Verify Soniox on macOS

With the Milena system voice installed, run from the Desktop checkout:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
justrust build --local="Soniox acceptance requires macOS" -p jcode-desktop --target-dir target/verify-soniox
python3 scripts/accept-soniox-rt.py target/verify-soniox/debug/jcode-desktop target/soniox-proof
```

The runner sends a Russian PCM recording to a local WebSocket fixture through
Desktop's Soniox client. It saves native screenshots and timings for the first
word, a revision, and the final draft. It does not use the live API, a microphone,
or the installed daemon. Choose a new output directory for each run.

## SuperWhisper

Desktop does not reuse SuperWhisper credentials. You can use an ElevenLabs key
for live text, or run SuperWhisper as an external transcriber for the final transcript.
"Use SuperWhisper" (macOS, shown only when `/Applications/superwhisper.app`
exists) installs `~/.jcode/bin/superwhisper-dictate.sh` and selects it as the
command backend. The script opens `superwhisper://record/start`, waits for the next
folder in `~/Documents/superwhisper/recordings/`, and prints `meta.json`'s
`result`. SuperWhisper keeps ownership of the microphone, model, mode and
license. The square Stop voice button and Ctrl+Shift+V finish capture with
`superwhisper://record/stop` (SuperWhisper 2.16.3 or newer). The bridge receives
only the finished transcript, including when SuperWhisper uses a realtime model.
`SUPERWHISPER_RECORDINGS` and `SUPERWHISPER_TIMEOUT` (seconds, default 300)
override the defaults. At that limit, or when interrupted, the bridge stops
SuperWhisper and still waits (`SUPERWHISPER_STOP_GRACE`, default 45 s) for the
transcript instead of abandoning the recording. Desktop does not impose its own
deadline on the bridge. Select **Use SuperWhisper** again to update an existing bridge.
