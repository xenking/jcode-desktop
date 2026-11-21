# Open a swarm worker in another worktree

Run each surface in a fresh private macOS instance so both clicks must open a
worker panel rather than focus one already on screen.

```sh
for surface in sidebar status; do
  run="target/verify-swarm-$surface"
  python3 scripts/verify-jcode-desktop.py launch --run "$run" --fixture swarm-worktree || break
  python3 scripts/verify-jcode-desktop.py drive --run "$run" --journey "swarm-$surface"
  result=$?
  python3 scripts/verify-jcode-desktop.py cleanup --run "$run"
  test "$result" -eq 0 || break
done
```

The fixture creates two checkouts of one repository and assigns the running
worker record to the linked checkout. The journey captures the rendered sidebar
or open status list, clicks the worker through GPUI, and requires exactly one
focused panel for its session ID. The status journey also checks that the list
closes after the click. Inspect the before/after PNGs and JSON in each run.

This exercises GPUI layout, hit testing and production click handlers. It does
not inject operating system mouse events or start a Jcode daemon or a worker.

## Direct messages in the transcript

Use `launch --fixture swarm-dm --window-width 420 --jcode-bin "$JCODE" --bridge-bin "$BRIDGE"`
with binaries from the sibling checkout, then `drive --journey swarm-dm` and
`cleanup` on the same run. The journey sends a signed multiline DM through its
private daemon, clicks the summary twice and verifies that the draft survives.
Inspect `swarm-dm-collapsed.png` for a single elided line and
`swarm-dm-expanded.png` for the full Markdown. Repeat at a wider window width
with a fresh run. The notify delivery path does not write daemon history.
