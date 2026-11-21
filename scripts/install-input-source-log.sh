#!/bin/bash
set -euo pipefail
# Install a standalone, low-volume recorder. Re-run to update it.
root="$(cd "$(dirname "$0")/.." && pwd)"
available=$(df -Pk / | awk 'NR==2 {print $4}')
if (( available <= 30 * 1024 * 1024 )); then
  echo "Need more than 30 GiB free on / before compiling" >&2
  exit 1
fi
mkdir -p "$HOME/.jcode/bin" "$HOME/.jcode/logs" "$HOME/Library/LaunchAgents"
chmod 700 "$HOME/.jcode/logs"
binary="$HOME/.jcode/bin/input-source-log"
plist="$HOME/Library/LaunchAgents/dev.xenking.input-source-log.plist"
tmp="$binary.new.$$"
trap 'rm -f "$tmp"' EXIT
xcrun swiftc -O "$root/scripts/input-source-log.swift" -o "$tmp"
chmod 700 "$tmp"
mv -f "$tmp" "$binary"
# PlistBuddy handles XML escaping for arbitrary home directories.
/usr/libexec/PlistBuddy -c 'Clear dict' -c 'Add :Label string dev.xenking.input-source-log' \
  -c 'Add :ProgramArguments array' -c "Add :ProgramArguments:0 string $binary" \
  -c 'Add :RunAtLoad bool true' -c 'Add :KeepAlive bool true' "$plist" 2>/dev/null
launchctl bootout "gui/$(id -u)/dev.xenking.input-source-log" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$plist"
launchctl kickstart "gui/$(id -u)/dev.xenking.input-source-log"
echo "Installed dev.xenking.input-source-log. Log: $HOME/.jcode/logs/input-source.log"
