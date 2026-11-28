# Desktop bridge generation and lifetime

## Why restarting Desktop reused the old bundle

Desktop's SDK startup checked whether the shared `jcode-api.sock` existed and
returned before checking which executable served it. The bridge held a separate
`flock` beside that socket and accepted clients forever. Renaming an application
bundle does not replace the executable already mapped into a running process.
Consequently, a new Desktop reused an orphaned bridge from the renamed bundle.
Spawning another bridge at the shared path could not repair this because the
orphan still owned its lock.

The earlier identity check rejected an old bridge after connecting, but neither
replaced it nor changed the endpoint. Reconnect therefore kept finding the same
incompatible listener.

## Selection and replacement

Local Desktop connections now select a bundle-scoped endpoint using the pinned
companion's absolute path, build hash and executable file identity. A prior
bundle's global socket and lock cannot block that endpoint. Each attach checks
HelloOk's executable identity, build hash and these capabilities before sending
session requests:

- `bridge_lifecycle_v1`
- `peer_user_message_v1`

The SDK also compares the handshake's PID with the operating system's socket
peer PID. A listener at the selected endpoint is not sufficient proof of
readiness. A mismatched modern listener is retired, then replaced from the
selected bundle. A legacy listener that cannot retire is never killed merely
because its handshake was missing or slow.

After its own endpoint is ready, Desktop asks older sibling bridge endpoints to
retire. Pre-lifecycle bridges may reject this request, but cannot prevent the
new bundle from working.

## Graceful lifetime

A local bridge accepts this daemon-independent first frame:

```json
{"v":1,"id":1,"req":"retire_bridge"}
```

It responds with `{"v":1,"reply_to":1,"ev":"bridge_retiring"}`, withdraws its
listener and releases its lock, then drains already connected clients. The lock
file remains in place so two processes cannot lock different inodes under one
pathname. The draining predecessor never unlinks a successor's socket.

Parent death also withdraws the listener and drains its clients. A bridge with
no clients exits after 30 seconds even if its parent remains alive. Initial
clients have five seconds to provide a handshake, so an incomplete hello cannot
pin an otherwise idle bridge forever.

`retire_bridge_if_pid` checks the discovered PID on the same connection before
sending retirement. Timeout, unsupported legacy protocol and changed peer are
distinct SDK errors. Rollout can therefore fail closed rather than treating an
unresponsive live bridge as an absent process.

## Isolated acceptance

Build the changed CLI and Desktop harness test executable with `justrust`, then
run:

```bash
python3 scripts/accept-stale-bridge.py \
  --jcode /absolute/path/to/new/jcode \
  --old-jcode /absolute/path/to/old/jcode \
  --harness-test /absolute/path/to/jcode_desktop_harness-test-executable \
  --jcode-repo /absolute/path/to/jcode-checkout \
  --output "$JCODE_SCRATCH_DIR/stale-bridge-evidence"
```

The test copies all executables into private bundles and uses a private HOME,
state home, runtime directory and localhost inference provider. It exercises the
production Desktop startup, connection and session-worker path without opening
or focusing a GPUI window. It proves that:

1. A genuinely old copied bridge remains orphaned at the shared socket.
2. A modern wrong-bundle bridge occupying the new endpoint retires and is
   replaced by the correct bundle.
3. A second native client submits a prompt after Desktop attaches, and Desktop
   receives the live peer-prompt update without reloading history.
4. The new bridge exits after its Desktop parent exits.
5. A separate bridge with a live parent exits after 30 seconds without clients.

The fixture removes only its private processes and temporary state. It never
runs a rollout or modifies the installed application, daemon, configuration or
sessions. JSON evidence and logs remain under the requested output directory.
