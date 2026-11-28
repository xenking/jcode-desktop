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
    pub fn reason_text(&self) -> String {
        if self.reason.is_empty() {
            "no reason given".into()
        } else {
            super::named(&self.reason)
        }
    }
}

/// The popover's content: the chip's facts (two lines at most, the third line
/// of the popover is the Manage accounts action), or the daemon's answer to a
/// typed `/acct` command, which replaces them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutingPopover {
    reply: Option<Result<String, String>>,
}

impl RoutingPopover {
    pub fn show(&mut self, reply: Result<String, String>) {
        self.reply = Some(reply);
    }

    pub fn rows(&self, chip: Option<&RoutingChip>) -> Vec<Row> {
        match &self.reply {
            Some(Ok(text)) => text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| Row {
                    text: line.trim_end().to_owned(),
                    tone: Tone::Plain,
                })
                .collect(),
            Some(Err(error)) => error
                .lines()
                .map(|line| Row {
                    text: line.to_owned(),
                    tone: Tone::Warn,
                })
                .collect(),
            None => chip
                .map(RoutingChip::lines)
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
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip() -> RoutingChip {
        RoutingChip::new(
            "claude:claude-fox".into(),
            "claude:claude-fox resets soon and its room would be lost".into(),
            vec!["budget low".into()],
        )
        .unwrap()
        // The account's provider has other accounts, so the daemon sent an icon.
        .with_icons(|_| Some("🦊".into()))
    }

    #[test]
    fn a_moved_session_gets_the_account_and_one_reason_in_plain_words() {
        let mut moved = chip();
        moved.moved_from = Some("claude:claude-otter".into());
        let rows = RoutingPopover::default().rows(Some(&moved));
        let texts: Vec<_> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Running on 🦊 claude-fox",
                "Moved from claude-otter: claude-fox resets soon and its room would be lost",
            ]
        );
    }

    #[test]
    fn without_a_move_the_popover_says_only_where_the_session_runs_and_any_warning() {
        let rows = RoutingPopover::default().rows(Some(&chip()));
        assert_eq!(rows[0].text, "Running on 🦊 claude-fox");
        assert_eq!(rows[1].text, "⚠ budget low");
        assert_eq!(rows[1].tone, Tone::Warn);
        let mut calm = chip();
        calm.warnings.clear();
        assert_eq!(RoutingPopover::default().rows(Some(&calm)).len(), 1);
    }

    #[test]
    fn a_typed_acct_answer_replaces_the_facts_and_errors_are_flagged() {
        let mut popover = RoutingPopover::default();
        popover.show(Ok("routing: auto\n\n  claude-fox  subscription\n".into()));
        let texts: Vec<_> = popover
            .rows(Some(&chip()))
            .into_iter()
            .map(|row| row.text)
            .collect();
        assert_eq!(texts, ["routing: auto", "  claude-fox  subscription"]);
        popover.show(Err("unknown account `x`".into()));
        assert_eq!(popover.rows(None)[0].tone, Tone::Warn);
    }
}
