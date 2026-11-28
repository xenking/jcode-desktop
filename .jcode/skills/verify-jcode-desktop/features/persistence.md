# Preferences and workspace persistence

Users expect preference changes, session tabs, drafts and pending questions to
survive a relaunch without exposing private configuration or losing another
window's successful update.

## Sub-features

- persistence.config: preserve unrelated TOML and existing file permissions.
- persistence.concurrent: retain every successful concurrent preference edit.
- persistence.recovery: restore tabs, drafts and pending ask selections.
- persistence.ownership: reject stale writers, incompatible schemas and dead PTY handles.

## How to get to it (user POV)

Change a Desktop preference, arrange sessions and drafts, then quit and relaunch
the same profile. Recovery differs from hot reload, which retains host resources.
Source entry points are `crates/jcode-desktop-ui-core/src/config.rs`,
`crates/jcode-desktop-ui/src/workspace/recovery/workspace_recovery.rs` and
`crates/jcode-desktop-ui/src/workspace/lifecycle/workspace_restart.rs`.

## Driving it safely on macOS

The hidden helper uses a disposable fixture profile. Its composer journey does
not prove disk persistence. Use isolated regression tests for the storage path:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop-ui-core config::tests:: --target-dir target/verify-build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop-ui --lib workspace::recovery::tests:: --target-dir target/verify-build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop-ui --lib workspace::restart::tests:: --target-dir target/verify-build
```

Require permission preservation, stale-temporary-symlink isolation, concurrent
updates, restored pending questions and late-writer rejection. Retain the test
output. These tests use temporary files and headless GPUI, not the daily profile.

## Gotchas

- A process-level native quit/relaunch journey is still a harness gap. Do not
  claim it from an in-memory snapshot round trip or fixture screenshot.
- Do not delete or replace the user's config, recovery records or restart manifests
  to seed a test. Linux navigation with `--reloads 0` does not prove persistence.
- Crash recovery must discard dead host resource IDs. Hot reload must preserve
  resources in the still-running host. Check the [reload recipe](reload.md).
