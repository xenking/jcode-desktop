# Should Jcode Desktop speak A2UI?

**Yes for interoperability. No new UI framework for Jcode-only screens.** Jcode already renders declarative native applets. A2UI adds a portable contract with external agents and a compact authoring format with reusable macros. Start with one interactive card in chat. Do not embed CopilotKit, add a second GPUI dependency, or ship a Python interpreter.

## What to reuse

| Candidate | Decision | Evidence |
| --- | --- | --- |
| CopilotKit/generative-ui | Examples, not a Desktop dependency. AG-UI event transport, declarative A2UI, and embedded MCP Apps are different choices. | [Project's comparison and examples](https://github.com/CopilotKit/generative-ui). Desktop already has a session stream. |
| Official A2UI | Pin wire v1.0 and a catalog. Compile inference syntax on the producer side. Desktop only needs validated wire data. | [Protocol overview](https://a2ui.org/concepts/overview/), [canonical source snapshot](https://github.com/a2ui-project/a2ui/tree/5ff1b05f02d39f969b52a942e78e864ef3ac01fa), [formats](https://a2ui.org/concepts/inference-formats/). |
| Express + macros | Use the upstream TypeScript compiler under Bun. Register trusted macro implementations by name. The model supplies arguments, not executable JS. Expand before transport. Do not parse this DSL again in GPUI. | [TS inference compiler](https://github.com/a2ui-project/a2ui/tree/5ff1b05f02d39f969b52a942e78e864ef3ac01fa/typescript/agents/a2ui_agent/src/inference-formats/express), [macro semantics](https://a2ui.org/guides/macros/). At this snapshot the TS catalog-transformers directory contains pruning/base, not the Python `MacroExpander`. Distinguish a local macro registry from an upstream SDK feature. |
| Community `a2ui-gpui` | Reference only for this fork. Not drop-in. | [Crate 0.3.1](https://docs.rs/crate/a2ui-gpui/0.3.1). A scratch crate enabled `backend`, used Desktop's `[patch.crates-io] gpui` and local `gpui_macros`, then ran `justrust check`. `gpui-component 0.5.1` failed to compile against `be875c56`: e.g. `select.rs:704` calls `FocusHandle::focus(window)`, the fork requires `focus(window, cx)`. This tests this pinned combination, not every possible release or port. |
| Jcode native applets | Reuse their GPUI presentation and tool-card placement. Keep A2UI's own ID-indexed state outside the applet document. | [`applet_view.rs`](../../crates/jcode-desktop-ui/src/applet_view.rs) maps a validated document to themed GPUI elements and emits `ViewEvent`. [`docs/applet-api.md`](../applet-api.md) covers placement, capabilities, snapshots and the `applet` tool. |

## Smallest production slice

1. Offer a trusted `ReviewChange(title, actionLabel, changeId)` macro. Prompt only with signatures the connected client supports. Normalize each invocation into deterministic, unique component IDs; address model values and bind actions by ID. Limit input size and expansion depth/node count before allocating the full expansion. No shell, network or `eval` from model input.
2. Implement a pure Rust per-surface reducer. Keep the component map, data model, generation and stable row anchor keyed by host/session/surface. Upsert component definitions; retain unresolved children on partial delivery. Project a complete supported subtree to keyed native applet nodes; do not flatten bindings into stale text or claim unsupported variants work. For a first slice, accept literal text, stacks and buttons. Use a **Jcode-specific catalog** if the advertised Basic catalog cannot be honored. No arbitrary catalog URL fetch. Fall back to a readable text action when unsupported.
3. Mount a validated document in the existing inline applet slot. Keep Jcode styling, hit targets and focus order. Use an opaque applet action token resolved against a trusted surface record: never forward model-chosen `host.*` names to privileged applet actions. Send only fields requested by the action, not the entire applet state. Treat a click as intent, not authorization for tool use. Server checks generation and deduplicates retries; never dispatch a historical click on restore.
4. For an initial Jcode-owned producer, use the existing structured applet/session path and translate on the producer side if no other client needs the original wire data. For external A2UI streams or full two-way interoperability, retain raw wire at a negotiated boundary. Extend the sibling SDK/harness then, not on the assumption that a new transport is necessary. Likely sites in Jcode: `crates/jcode-harness-api/src/{events,requests}.rs`, `crates/jcode-sdk/src/client.rs`, `crates/jcode-harness-api-server/src/translate.rs`. Desktop routes sessions in `crates/jcode-desktop-harness/src/harness.rs` and dispatches applet actions in `crates/jcode-desktop-ui/src/workspace_applets.rs`.

**Ship gate:** model-generated card appears in a real transcript, retains input/focus through a streamed update, is operable from the keyboard, dispatches one correlated action and shows the returned update. Reopen the session and prove an old click cannot target a new generation. Check dark/light themes and assistive labels. Preserve a text route for clients that never advertised support. A compiled fixture and a simulated pointer click are not this production gate.

## Independent schema check

Against the pinned upstream checkout, no Python or server is required:

```sh
git clone --filter=blob:none --no-checkout https://github.com/a2ui-project/a2ui target/a2ui-spec
git -C target/a2ui-spec sparse-checkout set specification/v1_0 catalogs/basic/v1
git -C target/a2ui-spec checkout --detach 5ff1b05f02d39f969b52a942e78e864ef3ac01fa
(cd spike/a2ui-schema && bun install --frozen-lockfile --ignore-scripts)
bun spike/a2ui-schema/check.js target/a2ui-spec agent spike/a2ui-gpui/fixture.json
# Use `renderer` instead of `agent` for the emitted action. Use `-` for stdin.
```

Schema validity tests message shape, not graph integrity, renderer behavior, action delivery or protocol conformance. The test runner loads schemas from disk and does not resolve catalog URLs over the network.
