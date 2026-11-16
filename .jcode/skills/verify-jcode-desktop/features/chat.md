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
