#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo "error: macOS packaging must run on macOS" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/apple-network-retry.sh"
JCODE_REPO="${JCODE_REPO:-$ROOT/../jcode}"
VERSION="${VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)}"
BUILD_NUMBER="${BUILD_NUMBER:-${GITHUB_RUN_NUMBER:-1}}"
OUT="${OUT_DIR:-$ROOT/dist/macos}"
APP="$OUT/Jcode.app"
DMG="$OUT/Jcode-macOS-universal.dmg"
ZIP="$OUT/Jcode-macOS-universal.zip"
DMG_ROOT="$OUT/dmg-root"
ENTITLEMENTS="$ROOT/packaging/macos/Jcode.entitlements"
SPARKLE_ROOT="${SPARKLE_ROOT:-$OUT/sparkle}"
read -r -a TARGETS <<< "${PACKAGE_TARGETS:-aarch64-apple-darwin}"
# Extra cargo features for the Desktop build, comma separated. A Mac without
# the Metal toolchain passes `gpui_platform/runtime_shaders` so shaders compile
# when the app starts instead of when it is built.
DESKTOP_CARGO_FEATURES="${DESKTOP_CARGO_FEATURES:-}"

if [[ "${GITHUB_ACTIONS:-}" != true || "${RUNNER_ENVIRONMENT:-}" != github-hosted ]]; then
  admission=(python3 "$JCODE_REPO/scripts/build-admission.py")
  if [[ ! -r "${admission[1]}" && -n "${JCODE_BUILD_LEASE_FD:-}" && -n "${JCODE_BUILD_ADMISSION_PYTHON:-}" ]]; then
    admission=(python3 -c "$JCODE_BUILD_ADMISSION_PYTHON")
  elif [[ ! -r "${admission[1]}" ]]; then
    echo "build admission blocked: owner=desktop-package paths=$OUT,$JCODE_REPO policy=desktop: missing ${admission[1]}; update the Jcode source checkout" >&2
    exit 75
  fi
  admission+=(--owner desktop-package --kind desktop --units "${#TARGETS[@]}"
    --path "$OUT" --path "$SPARKLE_ROOT" --workspace "$ROOT/Cargo.toml" --workspace "$JCODE_REPO/Cargo.toml")
  if [[ -z "${JCODE_BUILD_LEASE_FD:-}" ]]; then
    exec "${admission[@]}" -- "$0" "$@"
  fi
  "${admission[@]}" --check-inherited
  unset JCODE_BUILD_ADMISSION_PYTHON
fi

# Keep local upgrades on the same signing identity so TCC grants survive.
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  APPLE_SIGNING_IDENTITY="$(/usr/bin/security find-identity -v -p codesigning 2>/dev/null |
    sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "Apple Development: .*"$/\1/p' | head -1)"
fi
IDENTITY="${APPLE_SIGNING_IDENTITY:--}"

# Local signed builds use the paired rollout. Public releases retain Sparkle.
if [[ -z "${JCODE_DESKTOP_PAIRED_ROLLOUT:-}" ]]; then
  JCODE_DESKTOP_PAIRED_ROLLOUT=0
  if [[ "${GITHUB_ACTIONS:-}" != true && "${REQUIRE_SECURE_UPDATES:-0}" != 1 && "$IDENTITY" != - ]]; then
    JCODE_DESKTOP_PAIRED_ROLLOUT=1
  fi
fi
case "$JCODE_DESKTOP_PAIRED_ROLLOUT" in
  0) ;;
  1) [[ "$IDENTITY" != - && "${REQUIRE_SECURE_UPDATES:-0}" != 1 ]] || {
       echo "error: paired rollout requires a stable Apple signing identity and no Sparkle feed" >&2
       exit 1
     } ;;
  *) echo "error: JCODE_DESKTOP_PAIRED_ROLLOUT must be 0 or 1" >&2; exit 1 ;;
esac
export JCODE_DESKTOP_PAIRED_ROLLOUT

# Only standalone Sparkle builds read the keypair from the 0600 env file.
SPARKLE_ENV="${SPARKLE_ENV:-$HOME/Library/Application Support/jcode/sparkle.env}"
if [[ "$JCODE_DESKTOP_PAIRED_ROLLOUT" == 0 && -z "${SPARKLE_PUBLIC_KEY:-}" && -r "$SPARKLE_ENV" ]]; then
  SPARKLE_PUBLIC_KEY="$(sed -n 's/^SPARKLE_PUBLIC_KEY=//p' "$SPARKLE_ENV")"
  SPARKLE_PRIVATE_KEY="${SPARKLE_PRIVATE_KEY:-$(sed -n 's/^SPARKLE_PRIVATE_KEY=//p' "$SPARKLE_ENV")}"
fi
RELEASES_BASE="${RELEASES_BASE:-https://github.com/xenking/jcode-desktop/releases/download}"
UPDATE_FEED_URL="${UPDATE_FEED_URL:-https://github.com/xenking/jcode-desktop/releases/download/desktop-latest/appcast.xml}"
BINS=(jcode-desktop jcode jcode-harness-api-bridge)

notarize() {
  local artifact="$1"
  local result submission_id status
  result="$(mktemp "${TMPDIR:-/tmp}/jcode-notary.XXXXXX")"
  if apple_network_retry xcrun notarytool submit "$artifact" --keychain-profile "$APPLE_NOTARY_PROFILE" \
      --wait --output-format json >"$result"; then
    cat "$result"
    rm -f "$result"
    return 0
  else
    status=$?
  fi
  cat "$result"

  submission_id="$(python3 - "$result" <<'PY'
import json
import sys

try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("id", ""))
except (OSError, json.JSONDecodeError):
    pass
PY
)"
  if [[ -n "$submission_id" ]]; then
    echo "Notarization failed for $(basename "$artifact"); fetching notarization log..." >&2
    xcrun notarytool log "$submission_id" --keychain-profile "$APPLE_NOTARY_PROFILE" || true
  fi
  rm -f "$result"
  return "$status"
}

# Release tags and beta labels are valid artifact versions, but Apple's bundle
# short version must be exactly three numeric components.
VERSION="${VERSION#desktop-v}"
VERSION="${VERSION#v}"
VERSION="${VERSION%%-*}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "error: VERSION must contain a three-part numeric macOS version" >&2
  exit 1
}
[[ "$BUILD_NUMBER" =~ ^[1-9][0-9]*$ ]] || {
  echo "error: BUILD_NUMBER must be a positive integer" >&2
  exit 1
}
export JCODE_DESKTOP_VERSION="$VERSION"

for command in cargo codesign ditto hdiutil iconutil lipo python3 rustup sips xcrun; do
  command -v "$command" >/dev/null || { echo "error: missing required command: $command" >&2; exit 1; }
done
[[ -f "$JCODE_REPO/Cargo.toml" ]] || { echo "error: set JCODE_REPO to a Jcode checkout" >&2; exit 1; }
[[ -f "$ENTITLEMENTS" ]] || { echo "error: missing entitlements: $ENTITLEMENTS" >&2; exit 1; }
if [[ -n "${APPLE_NOTARY_PROFILE:-}" && "${APPLE_SIGNING_IDENTITY:--}" == "-" ]]; then
  echo "error: notarization requires APPLE_SIGNING_IDENTITY" >&2
  exit 1
fi

export MACOSX_DEPLOYMENT_TARGET=13.0
for target in "${TARGETS[@]}"; do rustup target add "$target"; done
"$ROOT/scripts/fetch-sparkle.sh" "$SPARKLE_ROOT"

for target in "${TARGETS[@]}"; do
  cargo build --manifest-path "$ROOT/Cargo.toml" --release --target "$target" \
    ${DESKTOP_CARGO_FEATURES:+--features "$DESKTOP_CARGO_FEATURES"}
  cargo build --manifest-path "$JCODE_REPO/Cargo.toml" --release --target "$target" --bin jcode
  cargo build --manifest-path "$JCODE_REPO/Cargo.toml" --release --target "$target" \
    --package jcode-harness-api-server --bin jcode-harness-api-bridge
done

rm -rf "$APP" "$DMG_ROOT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks" "$OUT/icon.iconset"
PLIST_ARGS=(
  --template "$ROOT/packaging/macos/Info.plist.in"
  --output "$APP/Contents/Info.plist"
  --version "$VERSION"
  --build "$BUILD_NUMBER"
)
if [[ "$JCODE_DESKTOP_PAIRED_ROLLOUT" == 1 ]]; then
  PLIST_ARGS+=(--paired-rollout)
elif [[ -n "${SPARKLE_PUBLIC_KEY:-}" ]]; then
  PLIST_ARGS+=(--public-key "$SPARKLE_PUBLIC_KEY" --feed-url "$UPDATE_FEED_URL")
fi
if [[ "${REQUIRE_SECURE_UPDATES:-0}" == 1 ]]; then
  PLIST_ARGS+=(--require-updates)
fi
python3 "$ROOT/scripts/render-macos-plist.py" "${PLIST_ARGS[@]}"
ditto "$SPARKLE_ROOT/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
printf 'APPL????' > "$APP/Contents/PkgInfo"

for bin in "${BINS[@]}"; do
  if [[ "$bin" == jcode-desktop ]]; then base="${CARGO_TARGET_DIR:-$ROOT/target}"; else base="${CARGO_TARGET_DIR:-$JCODE_REPO/target}"; fi
  inputs=()
  for target in "${TARGETS[@]}"; do inputs+=("$base/$target/release/$bin"); done
  lipo -create "${inputs[@]}" -output "$APP/Contents/MacOS/$bin"
  chmod 755 "$APP/Contents/MacOS/$bin"
done

for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT/assets/app-icon/icon-1024.png" --out "$OUT/icon.iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$ROOT/assets/app-icon/icon-1024.png" --out "$OUT/icon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$OUT/icon.iconset" -o "$APP/Contents/Resources/Jcode.icns"
rm -rf "$OUT/icon.iconset"

# Use the hardened runtime without broad exceptions. The explicit empty
# entitlement set makes accidental privilege additions visible in review and in
# verify-macos-package.sh.
echo "package-macos: signing identity ${IDENTITY}" >&2
# A secure timestamp needs Apple's server and only matters for notarized
# Developer ID builds, so local identities sign without one.
TIMESTAMP=()
if [[ -n "${APPLE_NOTARY_PROFILE:-}" ]]; then TIMESTAMP=(--timestamp); fi
SIGN_ARGS=(--force --options runtime --entitlements "$ENTITLEMENTS" --sign "$IDENTITY" ${TIMESTAMP[@]+"${TIMESTAMP[@]}"})
# Sparkle's helper services need their own entitlements. Preserve those while
# re-signing every nested bundle with the host application's identity, then
# seal the framework. Avoid codesign --deep for signing because it can hide a
# malformed or incompletely signed nested bundle.
NESTED_SIGN_ARGS=(--force --options runtime --preserve-metadata=identifier,entitlements --sign "$IDENTITY" ${TIMESTAMP[@]+"${TIMESTAMP[@]}"})
# Autoupdate is a standalone Mach-O binary inside the framework rather than a
# bundle, so sealing Sparkle.framework does not replace its upstream signature.
# Sign it explicitly before the bundles that contain and seal it.
codesign "${NESTED_SIGN_ARGS[@]}" \
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate"
for nested in \
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Downloader.xpc" \
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Installer.xpc" \
  "$APP/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app"; do
  codesign "${NESTED_SIGN_ARGS[@]}" "$nested"
done
codesign "${NESTED_SIGN_ARGS[@]}" "$APP/Contents/Frameworks/Sparkle.framework"
for bin in jcode jcode-harness-api-bridge jcode-desktop; do
  codesign "${SIGN_ARGS[@]}" "$APP/Contents/MacOS/$bin"
done
codesign "${SIGN_ARGS[@]}" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

# Notarize and staple the application before creating the final archives. The
# temporary ZIP is only the transport accepted by notarytool.
if [[ -n "${APPLE_NOTARY_PROFILE:-}" ]]; then
  rm -f "$ZIP"
  ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  apple_network_retry xcrun stapler staple "$APP"
  apple_network_retry xcrun stapler validate "$APP"
fi

# A top-level Applications link gives Finder users the expected drag-to-install
# target. Copy the app so the source bundle remains available for local testing.
mkdir -p "$DMG_ROOT"
ditto "$APP" "$DMG_ROOT/Jcode.app"
ln -s /Applications "$DMG_ROOT/Applications"

rm -f "$ZIP" "$DMG"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
hdiutil create -volname "Jcode" -srcfolder "$DMG_ROOT" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$DMG_ROOT"

if [[ "$IDENTITY" != "-" ]]; then
  codesign --force ${TIMESTAMP[@]+"${TIMESTAMP[@]}"} --sign "$IDENTITY" "$DMG"
fi
if [[ -n "${APPLE_NOTARY_PROFILE:-}" ]]; then
  notarize "$DMG"
  apple_network_retry xcrun stapler staple "$DMG"
  apple_network_retry xcrun stapler validate "$DMG"
fi

EXPECT_NOTARIZED="$([[ -n "${APPLE_NOTARY_PROFILE:-}" ]] && echo 1 || echo 0)" \
  "$ROOT/scripts/verify-macos-package.sh" "$APP" "$DMG"
(cd "$OUT" && shasum -a 256 "$(basename "$ZIP")" "$(basename "$DMG")" > SHA256SUMS)
if [[ "$JCODE_DESKTOP_PAIRED_ROLLOUT" == 0 && -n "${SPARKLE_PRIVATE_KEY:-}" ]]; then
  UPDATES="$OUT/updates"
  ARCHIVE="Jcode-${VERSION}-macOS-universal.zip"
  rm -rf "$UPDATES" && mkdir -p "$UPDATES"
  cp "$ZIP" "$UPDATES/$ARCHIVE"
  printf '%s\n' "$SPARKLE_PRIVATE_KEY" | "$SPARKLE_ROOT/bin/generate_appcast" \
    --ed-key-file - --download-url-prefix "$RELEASES_BASE/desktop-v$VERSION/" "$UPDATES"
  grep -q 'sparkle:edSignature=' "$UPDATES/appcast.xml"
  cp "$UPDATES/appcast.xml" "$OUT/appcast.xml"
  echo "signed appcast: $OUT/appcast.xml"
fi
echo "macOS beta artifacts: $OUT"
