# Interaction latency profile

Current budgets, how to measure them, and the constraints that follow from past
measurements. Dated investigation records were cut; `git log -- PERFORMANCE.md`
holds them. Raw artifacts lived under `target/` and were never committed.

## Budgets

| Budget | Value | Where it comes from |
|---|---|---|
| Frame | 8.33 ms (120 Hz), 16.7 ms (60 Hz) | Live profiler counts draws above 8.33 ms; sustained physical-display 120 FPS is **unproven** |
| Interaction state response | p99 < 3.7 ms for focus, strip transition, panel move and resize preset | `interaction_latency_profile` (below), 32 panels in four strips |
| Visual settle | 150 ms for camera, row, order and width transitions; 180 ms for modal transitions | Motion policy, not CPU cost |
| View construction | 17.5 ms per action | Counted as missed in `JCODE_DESKTOP_PERF_ACTIONS` records; the self-development badge turns yellow when its wake-lag or construction budget is exceeded and red at twice the budget |
| Event bridge | at most 128 updates per turn, then yield 1 ms | Keeps a ready producer from monopolizing the UI executor |
| Transcript invalidation | unchanged repaint invalidates 0 rows; stable streaming text or reasoning invalidates at most 3 trailing rows | Structural, tool, history, selection and font changes invalidate fully |
| Session-row hover | only visible rows are constructed (virtual list, 100 px overscan); a 500-session hover measures fewer than 50 titles | `sidebar_hover_frame_profile` |
| OpenRig idle | session HUD polls fleet identity and seat activity every 10 s; a local HUD never enumerates peers | `scripts/accept-idle-openrig.py` |
| Main-thread hang report | watchdog heartbeat answered within 2 s; one report per stall and per minute; eight reports of at most 2 MiB kept in `$JCODE_HOME/logs/desktop-hangs` | `scripts/accept-main-hang.py` |
| Polling | workspace bridge and each embedded terminal drain at 16 ms (60 Hz); empty polls neither notify nor repaint | Replace with event-driven wakeups before lengthening the interval |

The production scheduler has no blanket 120 FPS cap. The Wayland backend follows
compositor frame callbacks, so an active window runs at the display's cadence.
GPUI throttles some unfocused animation to about 30 FPS and serious or critical
thermal state to about 60 FPS; those are not promises of delivered FPS (see
[the platform source audit](docs/display-refresh-scheduling.md)).

## Interaction latency profile

The opt-in `interaction_latency_profile` test drives the real key bindings
through GPUI into a workspace of 32 panels across four strips. It records 500
input-to-state samples after 50 warmups per operation, in three independent
runs with isolated state.

```sh
cargo test --release -p jcode-desktop-ui interaction_latency_profile -- --ignored --nocapture
cargo test --release -p jcode-desktop-ui loaded_animation_first_frame_profile -- --ignored --nocapture
```

Measured on 2026-08-22 (Dell XPS 13 9350, Linux x86_64, optimized build):

| Interaction | p50 range | p95 range | p99 range |
|---|---:|---:|---:|
| Horizontal focus | 0.82–1.31 ms | 0.92–1.79 ms | 1.10–1.95 ms |
| Vertical focus / strip transition | 1.31–2.25 ms | 1.40–3.54 ms | 1.69–3.66 ms |
| Horizontal panel move | 0.82–1.42 ms | 1.34–1.94 ms | 1.54–2.85 ms |
| Panel resize preset | 1.31–1.43 ms | 1.77–1.81 ms | 1.90–1.96 ms |

The numbers include key parsing, dispatch, workspace mutation, focus updates and
learning-state persistence, and exclude the keyboard, compositor, GPU
presentation and the animation frames after the state change. Perceived latency
is dominated by the settle policy, not handler cost: shortening 150/180 ms would
change snappiness far more than micro-optimizing handlers, which is a motion
decision. Vertical focus is the most expensive path (row selection, row
animation, coach update, two strips rendered). Coach-state writes run on a
coalescing worker, settled transcript rows are borrowed rather than cloned, and
panel entities are GPUI cache boundaries, so a strip translation does not
reparse unchanged transcript markdown.

A physical input-to-photon profile needs compositor presentation timestamps or a
high-speed camera; the headless profiler cannot provide one.

## Sustained camera navigation

The CPU-only navigation test delivers every animation frame of a pan, not only
the first one:

```sh
justrust test --release -p jcode-desktop-ui --lib navigation_frame_profile -- --ignored --nocapture
```

For real-GPU runs without touching the user's desktop, the suite uses a private
headless Wayland compositor (default output 2880x1800 at 120 Hz, scale 1.6;
override with `PERF_SUITE_MODE` and `PERF_SUITE_SCALE`):

```sh
python3 scripts/perf-suite.py target/perf/navigation-gpu \
  --display wayland --sway-prefix target/headless-sway-tools \
  --scenario navigate --retention on --panels 6 --panel-width 0.5 \
  --seconds 12 --repeats 3
```

Rules learned the hard way:

- Keep one virtual keyboard attached for the whole sampling interval. Short-lived
  keyboards leave the window inactive between keys and trigger GPUI's
  inactive-window throttle.
- Fixture widths of `1 / panel_count` do not necessarily move the camera. Use
  `--panel-width` presets and require changing camera targets in the record; do
  not compare focus-only narrow-panel numbers with camera-pan numbers.
- Alternate baseline and candidate runs, record system load, and discard an
  unpaired first run. Concurrent builds on the shared machine moved FPS by tens of
  percent; a single pair cannot show a gain.
- A passive live capture without input or animation intervals only proves the
  diagnostic boundary is reachable. Report it as that.
- Xvfb with lavapipe is a CPU fixture; GPUI's Vulkan presenter needs DRI3, so use
  the headless Wayland path for presentation. A private compositor still does not
  prove physical scanout cadence.

## Live profiling

Self-development launches (`--hot-reload`) show a compact badge with the rolling
p95 event-loop wake lag, view construction time and the worst retained sample;
samples are bounded and the badge refreshes four times per second. Normal
launches omit it; set `JCODE_DESKTOP_PERF=1` to opt in. The badge also reports
GPUI's p95 draw and presentation intervals, presented FPS, input-to-frame
latency, coalesced input events and inputs that arrived during a draw. With
`JCODE_DESKTOP_STATE` set, the same values go to the public diagnostic state.

`JCODE_DESKTOP_PERF_ACTIONS=/path/to/actions.jsonl` appends one record per
meaningful focus-left/right/up/down action after its animation settles, with
draw, presentation and input p95 computed from the samples added during that
action, settle time, presented and constructed frame counts, and missed 17.5 ms
construction budgets. Edge no-ops are excluded. Presentation fields are
nullable: in isolated headless Sway the presentation histogram can stay empty,
and then `present_p95_ms` and `presented_fps` are `null` and
`presented_frame_count` is 0. That is not 0 FPS; use the target compositor for
user-visible cadence.

The profiler emits interval-weighted mean draw and presentation times, window
focus, thermal state and a sampler identity, separates reload generations, warns
when animation samples are too sparse, and falls back to timing-only capture when
`perf` is missing. A target that exits mid-capture leaves partial evidence marked
incomplete.

Graphics selection matters independently of UI work. On the Intel test machine
hardware Vulkan needs the matching `vulkan-intel` package; UI activation logs
GPUI's actual GPU specs once, including software-emulation status, to the
persistent desktop diagnostic log. The screenshot harness forces lavapipe and
cannot prove hardware Vulkan presentation. `TUI_SLOW_FRAME` records in the Jcode
runtime log describe terminal sessions, not desktop window frames.

## Development-build runtime

The hot-reload workflow keeps Jcode Desktop's own crates and most dependencies at
`opt-level = 0` but builds a profile-selected list of runtime-hot dependencies
(taffy layout, GPUI, text shaping, the Wayland/Calloop event loop, the renderer)
at `opt-level = 2`. The animation registry and per-frame interpolation live in
`jcode-desktop-motion`, also `opt-level = 2`; the frequently edited GPUI
composition stays in the unoptimized, reloadable UI crate. Unoptimized
dependencies made the dev host use 16–40% of one core while idle; with the
selective list a 10-second steady-state sample in headless Sway used 3.7% (a full
`opt-level = 2` dependency build measured 3.0%).

## Compile times

Recorded in `target/build-timings.jsonl`. `python3 scripts/build-timings.py` takes
controlled measurements (touch a file, run the same cargo command as Ctrl+R,
record the median); the host also appends a `source: "host"` line for every real
Ctrl+R and startup build. `python3 scripts/build-timings.py --summary` prints the
log.

Baseline, 2026-09-22, XPS 14 (Core Ultra X9 388H, 16 threads), rustc 1.98.1, mold,
commit 4f6372c:

| Profile | Scenario | Median |
|---|---|---|
| dev | no-op | 0.6s |
| dev | touch a UI leaf file | 6.1s |
| dev | touch `workspace.rs` | 5.5s |
| dev | touch host `main.rs` | 5.4s |
| release | no-op | 0.8s |
| release | touch a UI leaf file | 48.1s |
| release | touch host `main.rs` | 49.2s |

The running hot-reload hosts are release builds, so every Ctrl+R costs about 48s,
nearly all of it optimized code generation for one large crate (ThinLTO 18.4s,
LLVM codegen and passes). `CARGO_PROFILE_RELEASE_INCREMENTAL=true
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=256` brings a UI-leaf rebuild to 12–15s after
one cold rebuild (about 7 minutes) and keeps the ABI, but yields less optimized UI
code, so it could apply only to Ctrl+R builds; that needs a host change
(`rebuild_ui`) and a host restart and is not adopted.

A dev-profile function-body edit in `jcode-desktop-ui` takes about 4.3s in rustc,
mostly fixed per-crate work (metadata 1.2s, monomorphization 0.5s, macro expansion
0.4s, cdylib link 0.45s, incremental save 0.4s); only splitting the crate reduces
it. A full `cargo build` Ctrl+R also rebuilds `jcode-base` (6.7s) when the sibling
Jcode checkout changes and relinks the host (2s) unless `--lib` is passed.
Optimizing proc macros (`[profile.dev.build-override] opt-level = 3`) changes the
UI crate's metadata hash, which a running hot-reload host rejects, and scoping it
to `[profile.test.build-override]` costs more rebuilds than it saves; neither is
enabled.

## Unit test run time

`cargo test -p jcode-desktop-ui --lib` takes about 20s wall but 200s of CPU; the
parallel run is contention-bound inside one process. `cargo nextest run` gives
each test its own process and finishes in 12–15s (config in
`.config/nextest.toml`). Unit tests ignore `~/.jcode/config.toml` unless
`JCODE_DESKTOP_CONFIG` is set. Theme switches are instant under `cfg(test)`
because the 180 ms fade runs on wall-clock time, which GPUI's test executor cannot
advance; tests that change the process-global theme hold
`theme::test_theme_lock()`. About a dozen tests still `thread::sleep` through real
transitions and can flake under heavy load; a test-controllable animation clock
would make them deterministic. `prompt_background::rounded_union` keeps a small
position-independent LRU of tessellated paths.
