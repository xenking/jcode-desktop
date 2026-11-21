# Executed proof receipt

Executed November 16, 2026 on macOS aarch64, in
`/Users/xenking/.jcode/wt/verify-jcode-desktop`, against the dirty source snapshot
based on `edc7686` including this change. This is native production GPUI with
existing offline fixtures, not a browser, mock renderer or visible user window.

## Reproduction

From the repository root, with a fresh evidence directory:

```sh
RUN=target/verify-desktop-proof-7
python3 scripts/verify-jcode-desktop.py launch --run "$RUN"
python3 scripts/verify-jcode-desktop.py doctor --run "$RUN"
python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --journey tools
python3 scripts/verify-jcode-desktop.py screenshot --run "$RUN" --name final.png
python3 scripts/verify-jcode-desktop.py cleanup --run "$RUN"
python3 scripts/verify-jcode-desktop.py cleanup --run "$RUN"
test -s "$RUN/tools-after.png" && test -s "$RUN/final.png" && test -s "$RUN/actions.jsonl"
```

The recorded build used `justrust build -p jcode-desktop --target-dir
/Users/xenking/.jcode/wt/verify-jcode-desktop/target/verify-build`, with Xcode's
DEVELOPER_DIR and SDKROOT. Binary SHA-256:
`a822758edb504a908592fe98df3b59862b4a0fd0b6d0955619f80701857461bd`.

## Observed result

- Doctor healthy; own child PID `51844`, exact window `51844:4294967297`, session
  `screenshot-fixture`. The private environment/profile/control socket were
  recorded in `isolation.json`/`run.json`. No existing app or bridge was selected.
- `actions.jsonl` records a fixed draft, initial state/capture, selector visibility
  wait and `click tool:expandable-title`, then resulting state/capture.
  `expanded_regions` changed only for that tool. The draft survived unchanged.
- The 1600x1902 before/after PNGs were opened and visually inspected. Before had
  two collapsed tool rows. After disclosed the first command and its test output;
  the second row stayed collapsed. These command/output strings are fixture data,
  not evidence that the displayed shell commands executed.
- Before: `tools-before.png`, 179331 bytes,
  SHA-256 `2a536e6ba47254fc78bc78c0ebb2af0c08fa39c36cd566b47be9a3b0314672d6`.
- After: `tools-after.png`, 357453 bytes,
  SHA-256 `b91855c14464432d2acf5580a4e40e763ee49434c704d5f5bccaf0aa26fbdae6`.
- Cleanup waited for its retained child, recorded exit `-15`, removed only its
  temporary root, and returned the same receipt on repetition. PNGs, semantic
  states, `tools-proof.json`, `final.png`, action log, runtime logs and receipts
  remained under `target/verify-desktop-proof-7/` after cleanup.
- Private runtime log inspection showed startup/memory entries, without a
  connection attempt. Private state/crash-recovery files were copied into
  evidence before removing the profile. This is not packet-level network proof.

Evidence is intentionally local and ignored by Git. Preserve this worktree's
`target/verify-desktop-proof-7/` or attach that directory to a review before
removing the worktree. The committed receipt is not a substitute for its PNGs.

## Regression discovered by the skill

The original hidden-window control path returned a retained stale scene after a
successful semantic action. Do not trust a state-only pass. The strengthened
`accept-native-control.py` failed on an unchanged binary with
`hidden render returned a stale frame after the composer changed`
(`target/native-render-red.log`). Drawing a fresh frame outside the Workspace
borrow before capture fixed the real path. The same native acceptance passed
(`target/native-render-green-final.log`). Assertions were not weakened.

An earlier `all` seed had every tool offscreen and correctly failed. Its cleanup
retained evidence. The existing compact `expandable` seed supplies a visible tool.
Two early build failures started no Desktop process. Apple's Python-injected
CommandLineTools SDKROOT was the cause; launch now resolves Xcode's SDK explicitly.

## Checks and remaining gaps

- 16 helper safety tests passed, including an unrelated sentinel process staying
  alive during cleanup and screenshots surviving it. Log: `target/verify-helper-tests.log`.
- Nine targeted Rust control tests passed: `target/verify-control-tests-final.log`.
- Affected UI clippy exited 0 with pre-existing warnings, including dependency
  `map_identity` and existing dead code: `target/verify-clippy-final.log`.
- Changed Rust file passes rustfmt. Workspace-wide formatting also reports
  unrelated existing diffs, including sibling Jcode files; they were not swept.
- Full workspace test attempted: 2158 passed, eight failed, 19 ignored before
  fail-fast. `target/verify-workspace-tests.log` retains every failure. Repeating
  with the sole production change removed reproduced the same eight failures;
  baseline with `--no-fail-fast`: 2285 passed, eight failed, 20 ignored
  (`target/verify-workspace-baseline.log`). Failures: terminal EOF tail, remote
  set-model/cancel ordering, workflow cancellation, diff preview handle leak,
  and four preview error fixtures. They remain outside this verification change.
- Only the mapped semantic tool-expansion entry point is accepted here. OS
  keyboard/mouse dispatch, token-button hit testing, clipboard, auth, inference,
  durable provider history, review entry points, terminal and workspace input
  are not covered by this proof. Linux recipes are mapped, not executed here.
- Hidden mode requires a macOS graphical session. The helper refuses other hosts
  or a checkout without the reviewed isolation gate. A SIGKILLed supervisor
  cannot be safely recovered by trusting a recorded PID; it refuses that cleanup.

Use `/maintain-verification-skill` when feature paths or harness contracts change.
