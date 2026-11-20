# Desktop keyboard audit, 2026-11-20

Scope: the fork ledger in `../jcode/docs/FORK_LEDGER.md` and Desktop changes through `3258cfc`. This compares the actual Tab path against the old pointer path. `Y/N` below means **Tab reachable / Enter or Space activates / visible keyboard focus**. `—` means not applicable. An implementation review is not runtime evidence.

| Control | Before | After | Notes and evidence |
| --- | --- | --- | --- |
| Live panel tabs, × | N/N/N | Y/Y/Y | Standard focus ring. The × is a separate stop when it fits in the visible cap. Compressed tabs may hide ×; the existing close-panel shortcut remains. Implementation and UI suite. |
| Sidebar session rows | N/N/N | Y/Y/Y | Keyboard uses the existing activation path, pointer clicks retain drag and multi-selection. Implementation and UI suite. |
| Sidebar todo count | N/N/N | Y/Y/Y | Opens the same focused orchestration panel as the mouse. Implementation and UI suite. |
| Other sidebar controls | Mixed | Mixed | New session and the orchestration entry use shared buttons. The sections roller and other toolbar links still need an individual pass. |
| Composer editor | Programmatic / Y / caret | Y/Y/caret | No new bindings. Without a visible completion, Tab traverses in both full workspace and single-panel mode. Existing ghost/slash completion consumes plain Tab while visible. Native single-panel and GPUI regression. |
| Model, effort, mode, microphone pills | N/N/N | Y/Y/Y | Shared focus handle/ring; model and effort return focus on Esc. Mode grouping and its existing plain-Tab toggle are unchanged. GPUI and native fixture (microphone not activated: no microphone access). |
| Model/effort/mode lists | Programmatic / Y / highlighted row | Same, plus shared dismiss | Up/Down and Enter remain; Esc returns to the pill, blur/outside click dismiss without stealing newly moved focus. Inline effort marks in a model result are still pointer subtargets. |
| Swarm pill and popup rows | N/N/N | Y/Y/Y | Esc returns to the pill; Up/Down navigates worker rows. GPUI outside click/Esc regression; native pill Tab/Space/Esc. |
| Attachment chips and preview | Editor only / pointer preview / N | Editor only plus preview action stops | Chips remain atomic editor content (caret navigation and Backspace), not separate Tab stops. Preview actions have focus rings and support closing back to the editor. |
| Ask tabs, options, comment and footer | Tab switched question / custom keys / selected row | Y/Y/Y | Tab moves between controls, Left/Right moves between question tabs, Up/Down moves between options, Enter/Space activates. Existing direct number selection remains. GPUI and native fixture. |
| Compaction and memory dividers | N/N/N | Y/Y/Y | Only rows with detail become buttons. UI suite and implementation; virtual off-screen rows are not Tab stops. |
| Tool/prompt disclosures | N/N/N | Y/Y/Y for shared toggles | Shared disclosure buttons have a keyboard ring. Specialized cards and virtual rows still need individual native verification. |
| Pending/new session | Mixed | Mixed | Pending editor is reachable and sidebar New Session is a button. Retry, destination and connection controls still need a separate pass. |
| Orchestration | N/N/N | Y/Y/Y | Session cards and All sessions button share pointer/keyboard actions. This is a panel, not a modal. Implementation and UI suite. |

## Evidence and limits

- `justrust test -p jcode-desktop-ui --lib` and `justrust clippy -p jcode-desktop-ui --all-targets` exercise the code without using the installed Desktop or shared daemon.
- `verify-jcode-desktop.py` uses a hidden native macOS window and private offline fixtures. The allowlisted `fixture:` keys dispatch `KeyDown` and `KeyUp` to GPUI; this exercises the actual view/key routing, not macOS HID injection. Native focus movement must change the observable focus field, not merely produce a successful request.
- The rung test measures minimum mark-to-backing contrast across all 19 built-in palettes and nine effort levels: **3.50:1**, independent of the selected/hover/normal row paint. The backing is opaque, so the ratio does not change when the parent row changes color. This is measured paint-color contrast, not antialiased edge-pixel contrast.
