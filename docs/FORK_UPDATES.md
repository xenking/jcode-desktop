# Fork updates (xenking/jcode-desktop)

Apple Silicon installs from the fork's local rollout update the CLI, running
daemon and Desktop as one pair. Published macOS releases still use Sparkle.

## Update an installed pair

Choose **Jcode → Check for Updates…** (also in **Settings → About**, or run
`/update`). The dialog shows both installed versions, the exact target commits
from `origin/master` and `origin/main`, and a summary of the changes. **Build
and Update** confirms those commits, not whatever is at the branch heads later.

The bundled CLI prepares the pair using the existing rollout checkouts, build
cache, and Apple Development signing identity. The daemon waits for every turn
to finish before reloading or replacing Desktop. The app reopens on success.
Inspect a failed or interrupted install with `jcode rollout --status`.

Rollout needs at least 30 GiB free, and packaging may require more. The Desktop
updater reports a space shortage without deleting build caches. Keep the same
Apple signing identity for subsequent updates so macOS privacy grants carry
forward. Paired Desktop bundles do not load a Sparkle feed or use its keys.

Legacy installs need one normal `jcode rollout` to receive the paired updater
and its companion daemon. Do not pass `--now` if active turns must finish.

## Settings

| Where | Knob | Default |
|---|---|---|
| In-app updater (`jcode-desktop-updater/src/release.rs`) | env `JCODE_DESKTOP_RELEASES_URL` (runtime, or at build time to change the baked default) | `https://github.com/xenking/jcode-desktop/releases/download` |
| Sparkle feed (`scripts/package-macos.sh`) | env `UPDATE_FEED_URL` | `https://github.com/xenking/jcode-desktop/releases/download/desktop-latest/appcast.xml` |

The updater expects `<base>/desktop-latest/latest.json` and
`<base>/desktop-v<version>/` assets with `SHA256SUMS`, same layout as upstream.
The Sparkle feed is embedded only in non-paired builds with a
`SPARKLE_PUBLIC_KEY`. Local packs without a signing identity do not support
paired updates.

## Checks and release triggers

Each push to `main` and PR targeting `main` runs one read-only Linux job,
`Desktop contracts`. It checks the runtime commit pinned by the native release
workflows, then runs the script and release-policy tests. It does not build
the app or contact live release services. To run the tests locally:

```bash
python3 -m unittest discover -s tests -v
```

Native packaging and smoke tests stay in the macOS, Linux, Windows and FreeBSD
release workflows. The release coordinator accepts only manual dispatch and
defaults to a dry run. A normal `main` push cannot create a release tag, access
signing credentials or start a native release build.
