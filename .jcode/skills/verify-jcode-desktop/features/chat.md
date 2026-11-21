# Chat composition and submission

Users draft prompts and submit them to a coding session without losing text or
attachments. A visible draft is not a real provider response.

## Sub-features

- chat.draft: entering text updates the visible composer.
- chat.enter: Enter submits an enabled non-command prompt.
- chat.send: the Send pill submits the same prompt.
- chat.startup: a first prompt survives startup delay/retry.

## How to get to it (user POV)

Open a session with workspace + or Cmd/Super+Enter, or focus an existing chat.
Type in its composer, then press Enter or click Send.

## Driving it with native-control

Preconditions: fresh healthy helper run, no production credentials. Its composer
is an offline fixture, not a provider-connected session.

- **Draft:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --journey composer`.
  Require exact text in `composer-after.json` and visibly in its PNG.
- **Arrows and stash:** on a fresh offline run, use `drive --run "$RUN" --journey composer-arrows`.
  Require `composer-arrows-proof.json` and the stash indicator in `composer-arrows-stashed.png`.
  This dispatches real GPUI key events to the hidden window, not macOS HID events.
- **Composer controls:** launch with `--fixture transcript-scroll`, then use
  `drive --run "$RUN" --journey composer-advanced`. Require
  `composer-advanced-proof.json`. It checks an embedded command after Escape,
  image draft/history round-trip and gallery visibility, resize by drag and
  Alt+Shift+Up/Down, reset by double-click, and scroll containment at the edge.
  It seeds the image without touching the system clipboard.
- **Enter:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action press_key --params '{"key":"Enter"}'`.
  Then `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action state`.
  Require `submitted: true` in the action reply and an empty composer.
- **Send:** set a new draft with
  `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action set_composer --params '{"text":"Send-path proof"}'`,
  then `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action click --params '{"selector":"composer.send"}'`.
  Require the same submission/empty-composer result. Capture
  `python3 scripts/verify-jcode-desktop.py screenshot --run "$RUN" --name chat-sent.png`.
- **Native first-prompt input, Linux:**
  `python3 scripts/screenshot.py target/chat-fresh.png --binary target/debug/jcode-desktop --no-build --transcript empty --fresh-interact`.
  This existing private-display journey checks actual typing/submission visually.

## Gotchas

- Semantic Enter/Send do not prove keyboard dispatch or the Send hitbox.
- Offline submission does not prove an SDK request, persisted history, provider
  auth, inference or an assistant response. A real-daemon journey needs private
  sockets, test credentials and explicit cost scope.
- Queueing, attachments, commands and retry need separate journeys in
  `scripts/screenshot.py` and `scripts/accept-startup-submit.py`. A draft pass
  does not cover them. Missing data is a failed precondition.
- Queued-message editing: `launch --queue-edit` seeds two daemon-queued rows and
  opens the first in its inline editor, with a screenshot, a path-based file and
  an inline file. Capture after the row settles (about 3 s): the first frames
  lay out one line, and image thumbnails load asynchronously. Wire behavior
  (byte-for-byte `EditQueuedPrompt`, hold release) is pinned by GPUI tests in
  `panel_queue_tests.rs`, not by this screenshot.
