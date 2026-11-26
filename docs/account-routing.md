# Account routing in Desktop

Jcode can route each session onto one of several accounts under a configured
policy (`[routing]` in `config.toml`). Desktop shows what the runtime decided,
lets you answer a spending limit with one click, and steers routing without
leaving the session. Everything here needs a runtime that advertises the
`routing_v1` capability. On an older runtime none of it appears.
The policy itself (accounts, budgets, egress) is documented in the Jcode
repository: [ROUTING.md](https://github.com/xenking/jcode/blob/master/docs/ROUTING.md).

## Account chip

A pill in the composer's status row names the account the session runs on.
Hover it for the reason the policy chose it, the account a warm session will move
to at its next cache-cold turn, and any warnings. Every account leads with
its own icon (an animal by default, `icon` under `[[routing.accounts]]` overrides
it). A `⚠` after the name marks warnings (`⚠3` for three), and `→ name` marks a
pending move. Click it to open the account popover above the composer: the same
facts as the hover, then the daemon's `/acct status` table. Click again or press
`Esc` to close it. Routing text never enters the transcript: every `/acct` answer,
errors included, shows in the popover. When a decision moves the session to
another account, a one-line notice row says so once, with the reason.

## Spending-limit gate

When an account reaches a limit whose action is `ask`, the runtime holds the turn
and Desktop shows a card in place of the text box: the reason, then one row for
each way out and a final Cancel row.

| Row | Meaning |
| --- | --- |
| Switch to *account* | Run this turn on that account. |
| Wait until *time* | Hold the turn until the limit window resets. |
| Keep spending on *account* for 30m | Acknowledge the limit for 30 minutes. |
| Cancel | Drop the turn. |

Click a row, or use the keyboard, which matches the terminal UI: `Up` and `Down`
(or `Tab`) move, `Enter` answers the focused row, `1` to `9` answer a row
directly and `Esc` cancels. Focus starts on the first switch row. The card closes
when the runtime closes the gate, including when another client answered first.
If the answer is refused, the card stays with the reason so you can retry or
cancel. Subagents and non-interactive runs are never gated: they are blocked with
the reason instead.

## `/acct`

Type `/acct` in the composer.

```text
/acct [status]
/acct pin <account>|off        pin this session and its children
/acct switch <account>|off     move this session
/acct hold <account> [duration]|off   hold every interactive session
/acct ack <account> [duration] keep spending past an ask limit
/acct paid auto|off|<duration> allow paid accounts
/acct unstick                  clear the hold
```

Durations look like `30m`, `2h`, `1h30m` or `1d`. The answer shows in the account
popover, not the transcript. A bad command explains the usage there instead of
being sent.

## Model picker

A provider with several saved logins lists each login as its own group, led by the
account's icon. The group the session runs on says `active`. Choosing a model under
a group runs the session on that account. With `mode = "auto"` the runtime records
a switch to that account, so automatic routing does not move the session back until
you clear it with `/acct switch off`.

## Accounts sidebar

Below the account rows, the sidebar lists labelled API keys with their usage
windows and the budgets from `[[routing.budgets]]`.

- **Logins.** Each saved login of a provider has its own block, led by the
  account's icon.
- **API keys.** One row per labelled key, led by its icon (`provider · label`) and per key that
  reports a window, with an `active` marker on the key requests use. Each window
  is a `name · percent · reset` line over a bar, coloured like the account rows:
  amber from 70 percent, red from 90.
- **Budgets.** Name, scope and window, with `used / limit` in the budget's unit
  (`$` for dollars) and a bar of used over limit. The bar turns amber at the
  budget's `warn` level and red at its limit. A meter the runtime cannot read
  shows `unknown / limit` with no bar. It is never drawn as zero.

The view refreshes about once a minute from the runtime's `get_routing_view`
request and keeps the last good reading if the runtime is busy or restarting.
