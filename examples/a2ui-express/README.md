# Author a native button in A2UI Express

```text
root = GreetingButton("Say hello", "greet", "world")
```

`GreetingButton` is registered in a producer-only catalog. The published upstream `ExpressParser` checks Express syntax and compiles local components. A trusted JavaScript expander replaces each macro with Basic `Button` and `Text` nodes, retaining the macro's ID for the button. Two invocations can coexist inside a `Column`. No macro code, custom catalog, Python, or web renderer is sent to Desktop. The sample intentionally accepts a complete, unwrapped Express block with literal macro arguments, not an entire Markdown transcript or incremental updates.

```sh
cd examples/a2ui-express
bun install --frozen-lockfile
bun test
bun demo.js demo.express     # or pipe raw Express on stdin
```

The CLI prints a JSON array suitable for the [native single-button spike](../../spike/a2ui-gpui/README.md). Unknown macro names and generated ID collisions produce an error, not partial wire output. This is a custom expander, **not** a port of every upstream Python macro feature. The Bun lockfile pins published `@a2ui/agent` 0.1.0 and `@a2ui/web_core` 0.15.0. Those packages are not identical to the upstream Git revision used for schema verification.
