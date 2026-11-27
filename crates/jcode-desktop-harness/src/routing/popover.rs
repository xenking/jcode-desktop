//! What the account-routing popover shows: the chip's own facts, then the
//! daemon's answer to the last routing intent. Routing text never goes into the
//! transcript; this is where `/acct` replies and the chip's status land.

use super::RoutingChip;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Warn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    pub tone: Tone,
}

impl RoutingChip {
    /// Why the daemon chose this account, or a plain note when it gave none.
    pub fn reason_text(&self) -> &str {
        if self.reason.is_empty() {
            "no reason given"
        } else {
            &self.reason
        }
    }
}

/// The popover's content. `waiting` is true from a click until the daemon answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutingPopover {
    reply: Option<Result<String, String>>,
    waiting: bool,
}

impl RoutingPopover {
    /// Opened by a click: a status request is on its way.
    pub fn awaiting_status() -> Self {
        Self {
            reply: None,
            waiting: true,
        }
    }

    pub fn show(&mut self, reply: Result<String, String>) {
        self.reply = Some(reply);
        self.waiting = false;
    }

    /// One row per line: the chip's facts, then the reply. Blank reply lines
    /// are dropped, indentation and the `*` marker of the account table stay.
    pub fn rows(&self, chip: Option<&RoutingChip>) -> Vec<Row> {
        let plain = |text: String| Row {
            text,
            tone: Tone::Plain,
        };
        let mut rows: Vec<Row> = chip
            .map(RoutingChip::tooltip)
            .unwrap_or_default()
            .into_iter()
            .map(|text| Row {
                tone: if text.starts_with('⚠') {
                    Tone::Warn
                } else {
                    Tone::Plain
                },
                text,
            })
            .collect();
        match &self.reply {
            Some(Ok(text)) => rows.extend(
                text.lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| plain(line.trim_end().to_owned())),
            ),
            Some(Err(error)) => rows.extend(error.lines().map(|line| Row {
                text: line.to_owned(),
                tone: Tone::Warn,
            })),
            None if self.waiting => rows.push(plain("Reading the routing table…".into())),
            None => {}
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip() -> RoutingChip {
        RoutingChip::new(
            "claude:claude-fox".into(),
            "behind pace".into(),
            vec!["budget low".into()],
            None,
        )
        .unwrap()
        // The account's provider has other accounts, so the daemon sent an icon.
        .with_icons(|_| Some("🦊".into()))
    }

    #[test]
    fn rows_lead_with_the_chip_then_the_reply_and_flag_warnings_and_errors() {
        let mut popover = RoutingPopover::awaiting_status();
        let waiting = popover.rows(Some(&chip()));
        assert_eq!(waiting[0].text, "Running on 🦊 claude:claude-fox");
        assert_eq!(waiting.last().unwrap().text, "Reading the routing table…");

        popover.show(Ok(
            "routing: auto\n\n  🦊 claude:claude-fox  subscription\n".into(),
        ));
        let rows = popover.rows(Some(&chip()));
        let texts: Vec<_> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Running on 🦊 claude:claude-fox",
                "Why: behind pace",
                "⚠ budget low",
                "routing: auto",
                "  🦊 claude:claude-fox  subscription",
            ]
        );
        assert_eq!(rows[2].tone, Tone::Warn);
        assert_eq!(rows[3].tone, Tone::Plain);

        popover.show(Err("unknown account `x`".into()));
        assert_eq!(popover.rows(None)[0].tone, Tone::Warn);
    }
}
