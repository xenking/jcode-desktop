# Composer picker behavior

## Comparison on November 22, 2026

| Product | Evidence | What the user sees |
| --- | --- | --- |
| Codex app | The locally installed, signed `com.openai.codex` build `26.1117.41037` in `/Applications/ChatGPT.app`. The bundled `app.asar` contains the composer tier selector and English descriptions. [OpenAI speed guide](https://learn.chatgpt.com/docs/agent-configuration/speed). | The composer separates speed from reasoning. It selects Standard, Fast, or Ultrafast when available. The menu warns about higher usage. The docs also list account, workspace, and residency constraints. |
| Codex CLI | Local `codex --version` reports `0.166.0`. [OpenAI source at that release](https://github.com/openai/codex/blob/rust-v0.166.0/codex-rs/tui/src/chatwidget/service_tiers.rs) and [OpenAI command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli). | The CLI reads the model's advertised tiers, gates commands by feature, and saves the selection. Its Fast keybinding is unavailable during a turn or popup. `/model` can set effort and speed independently. |
| Cursor | [Cursor Models & Pricing](https://cursor.com/docs/models-and-pricing). Cursor was not found on this Mac. No mirror or screenshots from another site were used. | Cursor documents separate Fast variants and rates for some models. It fixes Fast off and effort at medium on Plus. The exact installed picker layout was not verified. |

## Jcode Desktop choices and limits

- The model and effort menus outline the active row. The keyboard highlight can move without changing the selection.
- The composer has one speed control. It shows only advertised acceleration, warns when entitlement is unknown, and does not treat a requested tier as a provider report. The selection applies to one draft or session. It does not update defaults for other drafts.
- Quota copy is qualitative. Model, credential, plan, and provider determine the actual cost and availability. The desktop does not estimate prices or infer unavailable tiers from model names.
- The existing model picker clones its catalog settings before it opens. Draft capability refresh also walks the catalog once to resolve the selected route. The speed picker does not clone or rank the catalog. Selecting a live tier requires the session worker to hear back from the runtime. These are source-level observations, not latency measurements.
- No inference request was sent to compare speed or quota. An offline fixture can prove what Desktop requests and displays, not whether a paid provider serves the tier.
