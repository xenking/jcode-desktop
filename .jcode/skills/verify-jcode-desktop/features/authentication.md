# Provider accounts and authentication

Users connect provider accounts, inspect connection health and select an account
without changing credentials on another machine. A saved key is not a working account.

## Sub-features

- auth.picker: `/login` opens the native account flow and preserves the draft.
- auth.remote: a remote session must not modify local credentials.
- auth.health: saved credentials, cached status and a live provider test are distinct.
- auth.recovery: invalid credentials and unavailable models expose recovery actions.

## How to get to it (user POV)

Enter `/login` in a session or open its Accounts surface. Choose a provider and
its sign-in method. A live connection test is a separate action. Source entry
points are `panel_login.rs`, `panel_login_accounts.rs`, `panel_login_steps.rs`,
`jcode-desktop-harness/src/accounts.rs` and `jcode-desktop-accounts-ui/src/connection.rs`
under `crates/`.

## Driving it safely on macOS

The hidden helper deliberately removes provider credentials and has no semantic
login selector. Its doctor reports auth as not exercised. Do not substitute the
user's Accounts window, saved keys or a real provider request.

Offline regression checks run through the production account UI and classification
logic without a login or inference request:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop-ui --lib login_ --target-dir target/verify-build
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer justrust test -p jcode-desktop-accounts-ui --target-dir target/verify-build
```

Require the remote-login isolation, draft preservation and connection-health
checks to pass. These are headless GPUI/unit tests, not OAuth acceptance.

## Gotchas

- Live OAuth, API-key persistence, callback delivery and credential refresh remain
  unmapped private-instance journeys. Use disposable test accounts and explicit
  network/cost scope before extending the harness. Never print credentials.
- A preview error, account list or successful composer submission proves neither
  a provider connection nor authorization for a particular model.
- Linux private-Xvfb chat/navigation recipes do not cover authentication.
