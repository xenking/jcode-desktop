//! One disclosure model for every text the app would otherwise cut off.
//!
//! A truncated line stays a single compact row while collapsed. Its toggle
//! reveals the complete text in place, so nothing in a card is final: tool
//! titles, tool errors, tool output, prompts, notices, background tasks and
//! swarm lines all expand with the same control and the same state.
//!
//! Keep the pure decisions (`preview`, `Open::for_row`, `expanded_body`) free
//! of GPUI so the disclosure can be tested without a window.

use super::*;

/// How much of a text a collapsed region shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Budget {
    chars: usize,
    lines: usize,
}

impl Budget {
    pub(crate) const fn new(chars: usize, lines: usize) -> Self {
        Self { chars, lines }
    }
}

/// A one-row tool title.
pub(crate) const TITLE: Budget = Budget::new(90, 1);
/// A tool error: the opening claim plus its first wrapped lines.
pub(crate) const ERROR: Budget = Budget::new(300, 4);
/// A notice or swarm line that reads as prose.
pub(crate) const NOTICE: Budget = Budget::new(200, 2);
/// A dense sidebar or strip label.
pub(crate) const LABEL: Budget = Budget::new(60, 1);

/// Expanded text taller than this scrolls inside the card instead of pushing
/// the rest of the transcript off screen.
pub(crate) const MAX_HEIGHT: f32 = 320.;
/// Expanded text longer than this many lines is painted up to this point and
/// the copy control carries the remainder, so one huge tool result cannot make
/// the transcript lay out millions of glyphs.
pub(crate) const MAX_LINES: usize = 400;

/// The prefix painted while collapsed, or `None` when the complete text fits.
pub(crate) fn preview(text: &str, budget: Budget) -> Option<&str> {
    let mut lines = 1;
    for (characters, (byte, ch)) in text.char_indices().enumerate() {
        if characters >= budget.chars || (ch == '\n' && lines >= budget.lines) {
            return Some(text[..byte].trim_end());
        }
        if ch == '\n' {
            lines += 1;
        }
    }
    None
}

/// The collapsed text with its trailing ellipsis, or `None` when it all fits.
pub(crate) fn collapsed(text: &str, budget: Budget) -> Option<String> {
    preview(text, budget).map(|prefix| format!("{}…", prefix.trim_end()))
}

/// The body painted inside an expanded region, and how many lines only the
/// copy control keeps.
pub(crate) fn expanded_body(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_LINES {
        return (lines.join("\n"), 0);
    }
    (lines[..MAX_LINES].join("\n"), lines.len() - MAX_LINES)
}

/// Which disclosure controls a transcript row currently has open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Open {
    /// The prompt bubble shows its complete markdown.
    pub(crate) prompt: bool,
    /// A tool card shows its complete title, error and output.
    pub(crate) card: bool,
    /// A stop, cache-miss or connection notice shows its complete detail.
    pub(crate) notice: bool,
    /// An error card shows the provider's raw message, or a background task
    /// and swarm line show their complete text.
    pub(crate) details: bool,
}

impl Open {
    /// The disclosure state a row paints from the region keys the panel holds.
    pub(crate) fn for_row(
        regions: &HashSet<String>,
        item: &Item,
        index: usize,
        prompt: bool,
    ) -> Self {
        let details = regions.contains(&key::details(index))
            || matches!(item, Item::BackgroundTask { task_id, .. } if regions.contains(&key::task(task_id)));
        Self {
            prompt,
            card: match item {
                Item::Tool { call_id, name, .. } => {
                    regions.contains(call_id) ^ {
                        name == "agentgrep" && crate::config::display().show_agentgrep_output
                    }
                }
                _ => false,
            },
            notice: regions.contains(&key::notice(index)),
            details,
        }
    }

    /// Packed into the low bits of a text-document fingerprint, so a settled
    /// row still notices when only its disclosure changed.
    pub(crate) fn bits(self) -> u64 {
        u64::from(self.prompt)
            | u64::from(self.card) << 1
            | u64::from(self.notice) << 2
            | u64::from(self.details) << 3
    }

    pub(crate) const MASK: u64 = 0b1111;
}

/// Region keys. A tool card is keyed by its call id, everything else by its
/// row, so state survives row movement and never collides.
pub(crate) mod key {
    pub(crate) fn notice(index: usize) -> String {
        format!("notice:{index}")
    }

    pub(crate) fn details(index: usize) -> String {
        format!("details:{index}")
    }

    pub(crate) fn task(task_id: &str) -> String {
        format!("task:{task_id}")
    }

    pub(crate) fn worker(session_id: &str) -> String {
        format!("worker:{session_id}")
    }

    pub(crate) fn toast(id: &str) -> String {
        format!("toast:{id}")
    }
}

/// Every disclosure key the given rows can paint. A screenshot run uses it to
/// start with all of them open, so the capture shows what an expansion looks
/// like without replaying a click.
pub(crate) fn all_regions(items: &[Item]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Tool { call_id, .. } => {
                keys.insert(call_id.clone());
            }
            Item::Error(_) | Item::RuntimeError { .. } => {
                keys.insert(key::details(index));
            }
            Item::Stopped(_) | Item::CacheMiss(_) => {
                keys.insert(key::notice(index));
            }
            Item::BackgroundTask { task_id, .. } => {
                keys.insert(key::task(task_id));
            }
            _ => {}
        }
    }
    keys
}

/// Screenshot runs may ask for the expanded state up front.
pub(crate) fn screenshot_expanded() -> bool {
    std::env::var("JCODE_DESKTOP_SCREENSHOT_EXPANDED").as_deref() == Ok("1")
}

/// Flip one region open or closed and report the new state.
pub(crate) fn flip(regions: &mut HashSet<String>, key: &str) -> bool {
    if regions.remove(key) {
        false
    } else {
        regions.insert(key.to_owned());
        true
    }
}

/// The disclosure control. Every truncated surface uses it, so one gesture and
/// one set of words expands a prompt, a tool card and a notice.
pub(crate) fn toggle(
    id: SharedString,
    expanded: bool,
    closed: &'static str,
    open: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    let selector = id.clone();
    crate::keyboard::button(div().id(id))
        .debug_selector(move || selector.to_string())
        .flex_none()
        .px_2()
        .py(px(1.))
        .rounded_full()
        .bg(theme.INLINE_CODE_BG)
        .hover(|el| el.bg(theme.TOOL_BORDER))
        .cursor_pointer()
        .text_size(px(10.))
        .line_height(px(14.))
        .text_color(theme.TEXT_DIM)
        .on_click(on_click)
        .child(if expanded { open } else { closed })
}

/// A chevron-only control for rows too dense for words.
pub(crate) fn glyph_toggle(
    id: SharedString,
    expanded: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    let selector = id.clone();
    crate::keyboard::button(div().id(id))
        .debug_selector(move || selector.to_string())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(12.))
        .rounded_full()
        .hover(|el| el.bg(theme.TOOL_BORDER))
        .cursor_pointer()
        .text_size(px(9.))
        .line_height(px(12.))
        .text_color(theme.TEXT_FAINT)
        .on_click(on_click)
        .child(if expanded { "▾" } else { "▸" })
}

/// Expanded text in a bounded, scrollable region so a long tool result stays
/// inside its card.
pub(crate) fn scrolled(
    id: SharedString,
    max_height: f32,
    body: gpui::AnyElement,
) -> impl gpui::IntoElement {
    let selector = id.clone();
    crate::transcript_scroll::vertical(
        div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .w_full()
            .min_w_0()
            .max_h(px(max_height))
            .child(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_short_text_and_cuts_long_text_at_a_char_boundary() {
        for text in ["", "Short", "1\n2\n3\n4"] {
            assert_eq!(preview(text, ERROR), None);
        }
        assert_eq!(preview(&"x".repeat(300), ERROR), None);
        assert_eq!(
            preview(&"x".repeat(301), ERROR),
            Some("x".repeat(300).as_str())
        );
        // Never split a multi-byte character.
        let unicode = "🦀日本語".repeat(200);
        let prefix = preview(&unicode, ERROR).unwrap();
        assert_eq!(prefix.chars().count(), 300);
        assert!(unicode.starts_with(prefix));
        // Line budgets count wrapped source lines, not characters.
        assert_eq!(preview("1\n2\n3\n4\n5", ERROR), Some("1\n2\n3\n4"));
        assert_eq!(preview("only\ntwo", NOTICE), None);
        assert_eq!(preview("a\nb\nc", NOTICE), Some("a\nb"));
    }

    #[test]
    fn collapsed_marks_the_cut_with_an_ellipsis() {
        assert_eq!(collapsed("short", TITLE), None);
        assert_eq!(
            collapsed(&format!("{}   tail", "word ".repeat(30)), TITLE)
                .unwrap()
                .chars()
                .last(),
            Some('…')
        );
    }

    #[test]
    fn expanded_body_paints_a_page_and_keeps_the_rest_for_copy() {
        let short = "a\nb";
        assert_eq!(expanded_body(short), ("a\nb".to_string(), 0));
        let long: String = (0..MAX_LINES + 7).map(|n| format!("line {n}\n")).collect();
        let (body, hidden) = expanded_body(&long);
        assert_eq!(hidden, 7);
        assert_eq!(body.lines().count(), MAX_LINES);
        assert!(body.starts_with("line 0"));
        assert!(!body.contains(&format!("line {}", MAX_LINES)));
    }

    #[test]
    fn keys_never_collide_across_surfaces() {
        let keys = [
            key::notice(3),
            key::details(3),
            key::task("3"),
            key::worker("3"),
            key::toast("3"),
        ];
        assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len());
    }

    #[test]
    fn every_row_class_offers_its_region_for_a_screenshot_run() {
        let items = vec![
            Item::Tool {
                call_id: "call".into(),
                name: "bash".into(),
                input: "{}".into(),
                output: String::new(),
                done: true,
                error: None,
            },
            Item::Error("boom".into()),
            Item::BackgroundTask {
                task_id: "task".into(),
                label: "Label".into(),
                summary: "Summary".into(),
                detail: None,
                percent: None,
                done: true,
            },
            Item::Assistant("not expandable".into()),
        ];
        let keys = all_regions(&items);
        assert!(keys.contains("call"));
        assert!(keys.contains(&key::details(1)));
        assert!(keys.contains(&key::task("task")));
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn flipping_a_region_reports_its_new_state() {
        let mut regions = HashSet::new();
        assert!(flip(&mut regions, "call"));
        assert!(regions.contains("call"));
        assert!(!flip(&mut regions, "call"));
        assert!(regions.is_empty());
    }

    #[test]
    fn row_disclosure_reads_the_keys_the_row_actually_paints() {
        let tool = Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: "{}".into(),
            output: String::new(),
            done: true,
            error: None,
        };
        let mut regions = HashSet::new();
        assert_eq!(
            Open::for_row(&regions, &tool, 4, true),
            Open {
                prompt: true,
                ..Open::default()
            }
        );
        regions.insert("call".into());
        let open = Open::for_row(&regions, &tool, 4, false);
        assert!(open.card);
        assert_ne!(open.bits(), Open::default().bits());
        assert_eq!(open.bits() & !Open::MASK, 0);
        regions.insert(key::notice(4));
        assert!(Open::for_row(&regions, &tool, 4, false).notice);
        regions.insert(key::details(4));
        assert!(Open::for_row(&regions, &tool, 4, false).details);
        assert!(!Open::for_row(&regions, &tool, 5, false).notice);
    }
}
