# Click an Express-generated button in GPUI

Standalone feasibility test using Jcode Desktop's exact GPUI fork (`be875c56343dcbd45127a9805781a9e128057dd7`) and macros patch. It does not modify the installed Desktop. From this checkout on macOS with Xcode Metal tools:

```sh
mkdir -p target
(cd examples/a2ui-express && bun install --frozen-lockfile && bun test >&2 && bun demo.js demo.express > ../../target/compiled-surface.json)
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
export A2UI_FIXTURE="$PWD/target/compiled-surface.json"
justrust test --rerun --local='simulated macOS GPUI window' --manifest-path spike/a2ui-gpui/Cargo.toml -- --nocapture
# Optional: display an actual window instead of running the headless test.
justrust run --local='native macOS window' --manifest-path spike/a2ui-gpui/Cargo.toml -- "$A2UI_FIXTURE"
```

The headless test lays out the GPUI button, locates its bounds, simulates a pointer click and checks one emitted action. The action has name `greet`, surface `demo`, source `root` and context `{"recipient":"world"}`. Both the generated surface and observed action passed the pinned upstream schemas using the [independent check](../../docs/research/a2ui-gpui.md#independent-schema-check).

Only a root `Button`, literal `Text` child and literal event context are exercised. No webview or NSButton: the control is drawn and hit-tested by GPUI. This does not verify an OS window, physical input, accessibility, applet mounting, agent delivery, or pixel quality. It is not a general renderer for untrusted A2UI. The wider integration and ship gate are in the [research note](../../docs/research/a2ui-gpui.md).
