# Fork updates (xenking/jcode-desktop)

Upstream points at `1jehuang/jcode-desktop-releases` and `jcode.sh`. This fork
defaults to its own GitHub releases, and the practical path is a local build.

## Settings

| Where | Knob | Default |
|---|---|---|
| In-app updater (`jcode-desktop-updater/src/release.rs`) | env `JCODE_DESKTOP_RELEASES_URL` (runtime, or at build time to change the baked default) | `https://github.com/xenking/jcode-desktop/releases/download` |
| Sparkle feed (`scripts/package-macos.sh`) | env `UPDATE_FEED_URL` | `https://github.com/xenking/jcode-desktop/releases/download/desktop-latest/appcast.xml` |

The updater expects `<base>/desktop-latest/latest.json` and
`<base>/desktop-v<version>/` assets with `SHA256SUMS`, same layout as upstream.
The Sparkle feed is only embedded when `SPARKLE_PUBLIC_KEY` is set, so local
ad-hoc builds have no Sparkle auto-update at all.

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

## Recommended update flow (no releases published)

The fork publishes no release assets, so the in-app check finds nothing and is
harmless. Update by rebuilding from the sibling checkouts:

```bash
cd ~/projects/github.com/xenking/jcode && git pull
cd ../jcode-desktop && git pull
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
scripts/package-macos.sh            # -> dist/macos/Jcode.app (ad-hoc signed)
osascript -e 'quit app "Jcode"'      # let it save state
ditto dist/macos/Jcode.app /Applications/Jcode.app
```

The upstream app is backed up at `~/.jcode/migration/Jcode.app.upstream.bak`.
Upstream sync stays `git fetch upstream && git merge upstream/main`.
