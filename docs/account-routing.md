# Account routing in Desktop

Jcode can route each session onto one of several accounts under a configured
policy (`[routing]` in `config.toml`). Desktop shows what the runtime decided,
lets you answer a spending limit with one click, and steers routing without
leaving the session. Everything here needs a runtime that advertises the
`routing_v1` capability. On an older runtime none of it appears.
The policy itself (accounts, budgets, egress) is documented in the Jcode
repository: [ROUTING.md](https://github.com/xenking/jcode/blob/master/docs/ROUTING.md).

## Account chip

The pill in the composer's status row appears only when something needs a look:
the daemon moved the session to another account, or warns about its account. It
names the account by the name you gave it (never `provider:id`) and a `⚠` after
the name marks warnings (`⚠3` for three). With one account, or with several and
nothing to report, there is no pill: the model chip already leads with the serving
account's icon.

Click the pill to open the popover above the composer. It has at most three
lines: where the session runs, one plain reason when a move happened (otherwise
the first warning), and **Manage accounts**, which opens the accounts page. There
is no table, mode or config note in it. Click again or press `Esc` to close it;
once a move has been looked at the pill goes. Routing text never enters the
transcript: the full table is `/acct status`, whose answer (like every `/acct`
answer, errors included) replaces the popover's lines. When a decision moves the
session to another account, a one-line notice row says so once, with the reason.

## Auto-switch on the accounts page

The Auto-switch group of the accounts page is the pool: its order is the priority
(the first account is the default for new sessions, the rest take over in order
when one runs out or fails). Metered API keys stay in Manual until you drag them
in. The **On / Off** switch on the group turns every automatic move off: no
routing, rotation or failover; a session stays on its account and a blocked
account fails the request and names itself. The list is kept while it is off.

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
