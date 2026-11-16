//! Logical selectable text, independent of virtual-row layout and painting.
use super::*;
use std::hash::{Hash, Hasher};

use expandable::Open;

type Segments = Vec<(SharedString, SharedString)>;

#[derive(Default)]
pub(super) struct TranscriptTextDocument {
    rows: HashMap<usize, (u64, Segments)>,
    order: Vec<usize>,
    /// The items revision the settled rows were last fingerprinted at. While
    /// it holds, a settled row is the same item it was, so only rows built
    /// from live text, and disclosure changes, need hashing again. Streaming
    /// changes only those, many times a second.
    settled_revision: Option<u64>,
    display: Option<crate::config::DisplayConfig>,
}

impl TranscriptTextDocument {
    /// Return a replacement only when text, disclosure state, or row order
    /// changes. Settled markdown is not reparsed on pointer/animation frames.
    pub(super) fn sync(
        &mut self,
        items: &Transcript,
        rows: &[TranscriptRenderRow],
        expanded_prompts: &HashSet<(usize, bool)>,
        expanded_regions: &HashSet<String>,
    ) -> Option<Segments> {
        let display = crate::config::display();
        let preferences_changed = self.display.as_ref() != Some(&display);
        if preferences_changed {
            self.rows.clear();
            self.settled_revision = None;
            self.display = Some(display.clone());
        }
        let order: Vec<_> = rows.iter().map(|row| row.index).collect();
        let mut changed = preferences_changed || self.order != order;
        let settled_unchanged = self.settled_revision == Some(items.revision());
        for row in rows {
            let item = match &row.source {
                TranscriptRowSource::Settled(index) => &items[*index],
                TranscriptRowSource::Owned(item) => item,
            };
            let open = Open::for_row(
                expanded_regions,
                item,
                row.index,
                expanded_prompts.contains(&(row.index, false)),
            );
            let flags = open.bits();
            if settled_unchanged
                && matches!(row.source, TranscriptRowSource::Settled(_))
                && self
                    .rows
                    .get(&row.index)
                    .is_some_and(|entry| entry.0 & Open::MASK == flags)
            {
                continue;
            }
            // The low bits carry the disclosure flags so a settled row
            // skipped above still notices when it is expanded or collapsed.
            let fingerprint = (fingerprint(item, open) & !Open::MASK) | flags;
            if self
                .rows
                .get(&row.index)
                .is_some_and(|entry| entry.0 == fingerprint)
            {
                continue;
            }
            let segments = segments(item, row.index, open);
            self.rows.insert(row.index, (fingerprint, segments));
            changed = true;
        }
        self.settled_revision = Some(items.revision());
        if !changed {
            return None;
        }
        let retained: HashSet<_> = order.iter().copied().collect();
        self.rows.retain(|index, _| retained.contains(index));
        self.order = order;
        Some(
            self.order
                .iter()
                .flat_map(|index| self.rows[index].1.iter().cloned())
                .collect(),
        )
    }
}

fn fingerprint(item: &Item, open: Open) -> u64 {
    let mut state = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(item).hash(&mut state);
    open.bits().hash(&mut state);
    match item {
        Item::User(text) | Item::Assistant(text) | Item::Reasoning(text) | Item::Error(text) => {
            text.hash(&mut state)
        }
        Item::Tool {
            call_id,
            name,
            input,
            output,
            error,
            ..
        } => {
            call_id.hash(&mut state);
            name.hash(&mut state);
            input.hash(&mut state);
            output.hash(&mut state);
            error.hash(&mut state);
        }
        Item::BackgroundTask { label, summary, .. } => {
            label.hash(&mut state);
            summary.hash(&mut state);
        }
        Item::Stopped(notice) => {
            notice.detail.hash(&mut state);
            notice.failure.hash(&mut state);
            notice.retry.hash(&mut state);
        }
        Item::CacheMiss(notice) => notice.text().hash(&mut state),
        // A divider's text is selectable, so its state participates in the
        // document hash: expanding it must invalidate the cached document.
        Item::Compaction(row) => {
            row.content.hash(&mut state);
            row.state.hash(&mut state);
        }
        Item::Image(_) | Item::ResponseStats(_) | Item::Todos(_) => {}
        Item::Applet(instance) => instance.hash(&mut state),
    }
    state.finish()
}

fn segments(item: &Item, index: usize, open: Open) -> Segments {
    segments_with_display(item, index, open, &crate::config::display())
}

pub(super) fn segments_with_display(
    item: &Item,
    index: usize,
    open: Open,
    display: &crate::config::DisplayConfig,
) -> Segments {
    if display.screenshot_mode {
        match item {
            Item::Tool { .. } | Item::BackgroundTask { .. } | Item::Image(_) | Item::Error(_) => {
                return Vec::new();
            }
            Item::Stopped(notice) => {
                return vec![(
                    format!("{index}-notice-title").into(),
                    notice.title().to_owned().into(),
                )];
            }
            Item::CacheMiss(_) => {
                return vec![(format!("{index}-notice-title").into(), "Cache miss".into())];
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    let mut push = |key: String, text: String| {
        if !text.is_empty() {
            result.push((key.into(), text.into()));
        }
    };
    match item {
        Item::User(text) if jcode_sdk::todo::is_auto_poke_message(text) => {
            push(
                format!("{index}-auto-poke"),
                super::queue::auto_poke_notice_text(text),
            );
        }
        Item::User(text) => {
            let (text, _) = super::voice::tag::strip(text);
            let visible = if open.prompt {
                &*text
            } else {
                prompt::compact_prompt(&text).unwrap_or(&text)
            };
            return markdown::selection_segments(visible, index, false, true);
        }
        Item::Assistant(text) => return markdown::selection_segments(text, index, false, false),
        Item::Reasoning(text) => return markdown::selection_segments(text, index, true, false),
        Item::Tool {
            call_id,
            name,
            input,
            output,
            error,
            ..
        } => {
            push(format!("tool-name-{index}"), name.clone());
            push(format!("tool-summary-{index}"), tool_summary_full(input));
            if open.card {
                let mut detail = tool_detail(name, input, output);
                if let Some(error) = error {
                    detail.push_str("\n\nError\n");
                    detail.push_str(&strip_ansi(error));
                }
                let files = if display.diff_display != crate::config::DiffDisplay::Off {
                    diff_review::inline_result_files(
                        name,
                        input,
                        if error.is_none() { output } else { "" },
                    )
                } else {
                    std::sync::Arc::new(Vec::new())
                };
                for (file_index, file) in files.iter().enumerate() {
                    let key = diff_review::edit_preview_key(call_id, file_index);
                    push(format!("{key}-name"), name.clone());
                    push(
                        format!("{key}-intent"),
                        diff_review::edit_preview_intent(input),
                    );
                    push(format!("{key}-path"), file.path.clone());
                    if let Some(error) = error {
                        push(format!("{key}-error"), error.clone());
                    }
                }
                push(format!("tool-detail-{index}"), detail);
            }
        }
        Item::Error(message) => {
            push(
                format!("{index}-error"),
                recovery::native_error_message(message),
            );
            if open.details {
                push(format!("{index}-error-raw"), message.clone());
            }
        }
        Item::Stopped(notice) => {
            if notice.shows_error() {
                push(
                    format!("{index}-error"),
                    recovery::native_error_message(&notice.detail),
                );
            } else {
                push(format!("{index}-stop-reason"), notice.plain_detail());
            }
        }
        Item::BackgroundTask { label, summary, .. } => {
            push(format!("background-label-{index}"), label.clone());
            push(format!("background-summary-{index}"), summary.clone());
        }
        Item::CacheMiss(notice) => push(format!("{index}-kv-cache-miss"), notice.text()),
        Item::Compaction(row) => {
            push(format!("{index}-compaction-label"), row.label().to_string());
            if row.state.is_expanded()
                && let Some(detail) = row.detail()
            {
                push(format!("{index}-compaction-detail"), detail.to_string());
            }
        }
        // Pinned todos and non-text chrome are outside the transcript document.
        Item::Todos(_) | Item::Image(_) | Item::ResponseStats(_) | Item::Applet(_) => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn drag_observer_cancels_only_preexisting_momentum(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "selection-momentum".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            *panel.items = vec![Item::Assistant("Select this text".into())];
            panel
        });
        vcx.run_until_parked();
        let bounds = vcx.debug_bounds("selectable-text-0-0").unwrap();
        panel.update(vcx, |panel, _| {
            panel.transcript_wheel_glide.remaining = 123.
        });
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: bounds.center(),
            modifiers: gpui::Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.run_until_parked();
        let selection = panel.read_with(vcx, |panel, _| {
            assert!(panel.transcript_dragging);
            assert!(!panel.stick_to_bottom);
            assert_eq!(panel.transcript_wheel_glide.remaining, 0.);
            panel.transcript_selection.clone()
        });
        // Notifications caused by extending a selection must not discard new
        // wheel input received after the drag began.
        panel.update(vcx, |panel, _| {
            panel.transcript_wheel_glide.remaining = 456.
        });
        selection.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.transcript_wheel_glide.remaining, 456.);
        });
        selection.update(vcx, |selection, cx| {
            selection.finish();
            cx.notify();
        });
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| assert!(!panel.transcript_dragging));
    }

    fn rows(items: &[Item]) -> Vec<TranscriptRenderRow> {
        items
            .iter()
            .enumerate()
            .map(|(index, _)| TranscriptRenderRow {
                index,
                source: TranscriptRowSource::Settled(index),
                role: None,
                show_label: false,
            })
            .collect()
    }

    fn tool() -> Item {
        Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: r#"{"command":"printf hello"}"#.into(),
            output: "hidden output".into(),
            done: true,
            error: None,
        }
    }

    #[test]
    fn complete_document_caches_rows_and_excludes_collapsed_output() {
        let mut cache = TranscriptTextDocument::default();
        let items: Transcript = vec![
            Item::User("Prompt".into()),
            tool(),
            Item::Assistant("Answer".into()),
        ]
        .into();
        let rows = rows(&items);
        let prompts = HashSet::new();
        let mut tools = HashSet::new();
        let doc = cache.sync(&items, &rows, &prompts, &tools).unwrap();
        assert_eq!(doc.first().unwrap().0.as_ref(), "0-0");
        assert_eq!(doc.last().unwrap().0.as_ref(), "2-0");
        assert!(doc.iter().any(|(key, _)| key.as_ref() == "tool-summary-1"));
        assert!(!doc.iter().any(|(_, text)| text.contains("hidden output")));
        assert!(cache.sync(&items, &rows, &prompts, &tools).is_none());
        tools.insert("call".into());
        let expanded = cache.sync(&items, &rows, &prompts, &tools).unwrap();
        assert!(
            expanded.iter().any(
                |(key, text)| key.as_ref() == "tool-detail-1" && text.contains("hidden output")
            )
        );
        assert!(cache.sync(&items, &rows, &prompts, &tools).is_none());
        let trimmed = cache.sync(&items, &rows[..1], &prompts, &tools).unwrap();
        assert_eq!(trimmed.len(), 1);
        assert_eq!(cache.rows.len(), 1);
    }

    #[test]
    fn settled_rows_are_rehashed_only_when_the_transcript_changes() {
        let mut cache = TranscriptTextDocument::default();
        let mut items: Transcript =
            vec![Item::User("Prompt".into()), Item::Assistant("first".into())].into();
        let prompts = HashSet::new();
        let tools = HashSet::new();
        let rows_now = rows(&items);
        assert!(cache.sync(&items, &rows_now, &prompts, &tools).is_some());
        // Same transcript: nothing to replace.
        assert!(cache.sync(&items, &rows_now, &prompts, &tools).is_none());
        // An in-place edit bumps the revision, so the changed row is seen.
        if let Some(Item::Assistant(text)) = items.get_mut(1) {
            *text = "second".into();
        }
        let doc = cache.sync(&items, &rows(&items), &prompts, &tools).unwrap();
        assert!(doc.iter().any(|(_, text)| text.contains("second")));
        assert!(!doc.iter().any(|(_, text)| text.contains("first")));
        // Live text is an owned row and is always compared.
        let mut live = rows(&items);
        live.push(TranscriptRenderRow {
            index: 2,
            source: TranscriptRowSource::Owned(Box::new(Item::Assistant("partial".into()))),
            role: None,
            show_label: false,
        });
        let doc = cache.sync(&items, &live, &prompts, &tools).unwrap();
        assert!(doc.iter().any(|(_, text)| text.contains("partial")));
        live[2].source =
            TranscriptRowSource::Owned(Box::new(Item::Assistant("partial more".into())));
        let doc = cache.sync(&items, &live, &prompts, &tools).unwrap();
        assert!(doc.iter().any(|(_, text)| text.contains("partial more")));
    }

    #[test]
    fn prompt_document_matches_collapsed_preview_and_expansion() {
        let text = format!("{}\nTAIL", "first line\n".repeat(8));
        let collapsed = segments(&Item::User(text.clone()), 0, Open::default());
        assert!(!collapsed.iter().any(|(_, text)| text.contains("TAIL")));
        let expanded = segments(
            &Item::User(text),
            0,
            Open {
                prompt: true,
                ..Open::default()
            },
        );
        assert!(expanded.iter().any(|(_, text)| text.contains("TAIL")));
    }

    #[test]
    fn edit_tool_document_contains_raw_and_rich_text_only_when_open() {
        let edit = Item::Tool {
            call_id: "edit".into(),
            name: "edit".into(),
            input: r#"{"file_path":"src/main.rs","old_string":"before","new_string":"after"}"#
                .into(),
            output: "Applied".into(),
            done: true,
            error: None,
        };
        let closed = segments(&edit, 5, Open::default());
        assert!(!closed.iter().any(|(_, text)| text.contains("Applied")));
        let open = Open {
            card: true,
            ..Open::default()
        };
        let doc = segments(&edit, 5, open);
        assert!(
            doc.iter()
                .any(|(key, text)| key == "tool-detail-5" && text.contains("Applied"))
        );
        assert!(
            doc.iter()
                .any(|(key, text)| key == "tool-edit-edit-0-path" && text == "src/main.rs")
        );
        let moved = segments(&edit, 99, open);
        assert!(moved.iter().any(|(key, _)| key == "tool-edit-edit-0-path"));
        assert_ne!(
            fingerprint(&edit, Open::default()),
            fingerprint(&edit, open)
        );
    }

    #[test]
    fn multi_file_edit_document_keeps_each_header_in_order_with_stable_call_keys() {
        let mut tool = diff_review::fixture_items().pop().unwrap();
        let open = Open {
            card: true,
            ..Open::default()
        };
        let doc = segments(&tool, 12, open);
        let keys: Vec<_> = doc
            .iter()
            .filter(|(key, _)| key.starts_with("tool-edit-"))
            .map(|(key, _)| key.as_ref())
            .collect();
        assert_eq!(
            keys,
            vec![
                "tool-edit-diff-fixture-0-name",
                "tool-edit-diff-fixture-0-intent",
                "tool-edit-diff-fixture-0-path",
                "tool-edit-diff-fixture-1-name",
                "tool-edit-diff-fixture-1-intent",
                "tool-edit-diff-fixture-1-path"
            ]
        );
        let old = fingerprint(&tool, open);
        if let Item::Tool { call_id, .. } = &mut tool {
            *call_id = "replacement-call".into();
        }
        assert_ne!(old, fingerprint(&tool, open));
        assert!(
            segments(&tool, 12, open)
                .iter()
                .filter(|(key, _)| key.starts_with("tool-edit-"))
                .all(|(key, _)| key.contains("replacement-call"))
        );
    }
}
