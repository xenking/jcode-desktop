#!/bin/sh
# Jcode dictation bridge for a running SuperWhisper (macOS).
# Installed by Jcode Desktop Settings > Voice > "Use SuperWhisper" and used as
# `[dictation] command`. It asks SuperWhisper to record through its public
# deep link, waits for SuperWhisper to write the next recording, and prints
# that transcript to stdout. SuperWhisper owns the microphone, model, mode and
# license. Stop the recording with SuperWhisper's own shortcut or pill.
# No credentials are read and nothing is sent anywhere by this script.
set -eu
dir="${SUPERWHISPER_RECORDINGS:-$HOME/Documents/superwhisper/recordings}"
timeout="${SUPERWHISPER_TIMEOUT:-300}"
[ -d /Applications/superwhisper.app ] || { echo "SuperWhisper is not installed" >&2; exit 1; }
[ -d "$dir" ] || { echo "SuperWhisper recordings folder not found: $dir" >&2; exit 1; }
latest() { ls -1 "$dir" 2>/dev/null | grep -E '^[0-9]+$' | sort -n | tail -1; }
before="$(latest || true)"
open -g "superwhisper://record"
elapsed=0
while [ "$elapsed" -lt $((timeout * 2)) ]; do
  sleep 0.5
  elapsed=$((elapsed + 1))
  now="$(latest || true)"
  if [ -n "$now" ] && [ "$now" != "$before" ] && [ -f "$dir/$now/meta.json" ]; then
    text="$(/usr/bin/plutil -extract result raw -o - "$dir/$now/meta.json" 2>/dev/null || true)"
    if [ -n "$text" ]; then
      printf '%s\n' "$text"
      exit 0
    fi
  fi
done
echo "SuperWhisper produced no transcript" >&2
exit 1
