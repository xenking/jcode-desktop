# Reload and host lifecycle

Developers rebuild the UI without losing sessions, windows, drafts or host-owned
terminals. A failed candidate must leave the previous UI usable.

## Sub-features

- reload.handoff: snapshot and activate in the original native window.
- reload.rollback: restore the previous root after failed activation.
- reload.resume: preserve a failed resume snapshot for a later retry.
- reload.compatibility: reject incompatible ABI/schema before using candidate code.

## How to get to it (user POV)

The development host exposes Ctrl+R for rebuild/reload. Host or ABI changes can
require a coordinated restart instead. Source entry points are
`src/host/reload.rs`, `src/host/resources.rs` and
`crates/jcode-desktop-ui/src/workspace/lifecycle/workspace_restart.rs`.

## Driving it safely on macOS

Never send reload to the user's open app. The hidden verification helper builds a
private linked UI and its semantic key route supports only Enter/Escape. It does
not expose dynamic reload. Execute the headless host transaction harness instead:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop --bin jcode-desktop host::reload::tests:: --target-dir target/verify-build
```

Require same-window handoff, failed-activation rollback, failed-resume retry and
ABI validation. These exercise host transaction code with test roots, not a real
compiler-produced replacement library or an OS key press.

## Gotchas

- Rebuild acknowledgement is not proof of activation. A dynamic reload journey
  must record the loaded generation, unchanged window/session IDs, drafts and
  live PTY output before and after handoff, including a deliberately bad candidate.
- Linux navigation recipes with `--reloads 0 --linked-ui` intentionally skip
  reload. They cannot validate candidate loading or rollback.
- Live private dynamic-library reload remains a harness gap on macOS. Do not
  work around it with `desktop_selfdev reload`, the shared bridge, global input
  injection, process-name killing or the user's current window.
