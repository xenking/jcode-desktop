#!/bin/sh
# Jcode dictation bridge for a running SuperWhisper (macOS).
# Installed by Jcode Desktop Settings > Voice > "Use SuperWhisper" and used as
# `[dictation] command`. It asks SuperWhisper to record through its public
# deep link, waits for SuperWhisper to write the next recording, and prints
# that transcript to stdout. SuperWhisper owns the microphone, model, mode and
# license. Desktop stops capture with superwhisper://record/stop.
# When the recording limit is reached, or the caller interrupts this script,
# it stops SuperWhisper and still waits for the transcript, so a long
# dictation is delivered instead of abandoned.
# No credentials are read and nothing is sent anywhere by this script.
set -eu
dir="${SUPERWHISPER_RECORDINGS:-$HOME/Documents/superwhisper/recordings}"
timeout="${SUPERWHISPER_TIMEOUT:-300}"
grace="${SUPERWHISPER_STOP_GRACE:-45}"
[ -d /Applications/superwhisper.app ] || { echo "SuperWhisper is not installed" >&2; exit 1; }
[ -d "$dir" ] || { echo "SuperWhisper recordings folder not found: $dir" >&2; exit 1; }
latest() { ls -1 "$dir" 2>/dev/null | grep -E '^[0-9]+$' | sort -n | tail -1; }
before="$(latest || true)"
# Print the transcript of the recording made since start, if it is ready.
emit() {
  now="$(latest || true)"
  if [ -n "$now" ] && [ "$now" != "$before" ] && [ -f "$dir/$now/meta.json" ]; then
    text="$(/usr/bin/plutil -extract result raw -o - "$dir/$now/meta.json" 2>/dev/null || true)"
    if [ -n "$text" ]; then
      printf '%s\n' "$text"
      exit 0
    fi
  fi
}
# Stop recording, then give SuperWhisper time to finish transcribing.
finish() {
  open -g "superwhisper://record/stop" || true
  waited=0
  while [ "$waited" -lt $((grace * 2)) ]; do
    sleep 0.5
    waited=$((waited + 1))
    emit
  done
  echo "SuperWhisper produced no transcript" >&2
  exit 1
}
trap finish INT TERM
open -g "superwhisper://record/start"
elapsed=0
while [ "$elapsed" -lt $((timeout * 2)) ]; do
  sleep 0.5
  elapsed=$((elapsed + 1))
  emit
done
finish
