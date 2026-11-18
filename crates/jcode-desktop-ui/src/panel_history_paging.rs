//! Lazy expansion of the daemon's compacted transcript window.
use super::*;

const PAGE_SIZE: usize = 128;
const INITIAL_WINDOW: usize = 64;

#[derive(Clone, Debug, PartialEq, Default)]
struct HistoryRecovery {
    items: Vec<Item>,
    times: std::collections::HashMap<usize, i64>,
    requested_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct HistoryPaging {
    visible: usize,
    remaining: usize,
    anchor: Vec<Item>,
    #[serde(default)]
    bounded: bool,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    prompt_offset: Option<usize>,
    #[serde(skip)]
    pub(super) in_flight: bool,
    #[serde(skip)]
    recovery: Option<HistoryRecovery>,
}

impl HistoryPaging {
    pub(super) fn from_history(messages: &[jcode_sdk::HistoryMessage]) -> Option<Self> {
        // Older daemons expose this metadata as a system row. It is not a user
        // prompt and must not itself become a divider in the transcript.
        let marker = messages.iter().find(|m| {
            m.role == "system"
                && m.content.starts_with("Earlier conversation compacted - ")
                && m.content.contains("historical messages hidden")
        })?;
        let text = marker
            .content
            .trim_start_matches("Earlier conversation compacted - ");
        let remaining = text.split_whitespace().next()?.parse().ok()?;
        (remaining > 0).then_some(Self {
            visible: INITIAL_WINDOW,
            remaining,
            anchor: Vec::new(),
            bounded: false,
            cursor: None,
            prompt_offset: marker
                .content
                .split("Hidden user prompts: ")
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|s| s.parse().ok()),
            in_flight: false,
            recovery: None,
        })
    }

    pub(super) fn set_anchor(&mut self, items: &[Item]) {
        self.anchor = items.iter().take(3).cloned().collect();
    }

    pub(super) fn prompt_offset(&self) -> Option<usize> {
        self.prompt_offset.or_else(|| {
            ((self.bounded && self.cursor.is_none()) || (!self.bounded && self.remaining == 0))
                .then_some(0)
        })
    }

    fn prefix_len(&self, items: &[Item]) -> Option<usize> {
        if self.anchor.is_empty() {
            return None;
        }
        // Prefer the last match when prompts repeat. A window is cumulative,
        // and only the part preceding its unchanged old prefix is new.
        items.windows(self.anchor.len()).rposition(|window| {
            window
                .iter()
                .zip(&self.anchor)
                .all(|(left, right)| match (left, right) {
                    (Item::Image(left), Item::Image(right))
                        if matches!(
                            left.anchor,
                            Some(jcode_sdk::RenderedImageAnchor::UserPrompt { .. })
                        ) && matches!(
                            right.anchor,
                            Some(jcode_sdk::RenderedImageAnchor::UserPrompt { .. })
                        ) =>
                    {
                        left.media_type == right.media_type
                            && left.data == right.data
                            && left.label == right.label
                            && left.source == right.source
                    }
                    (Item::Compaction(left), Item::Compaction(right)) => {
                        left.content == right.content
                    }
                    (Item::Memory(left), Item::Memory(right)) => left.content == right.content,
                    _ => left == right,
                })
        })
    }
}

impl Panel {
    pub(super) fn request_older_history(&mut self) {
        let Some(paging) = &mut self.history_paging else {
            return;
        };
        if paging.bounded {
            if !paging.in_flight
                && let Some(before) = paging.cursor.clone()
            {
                paging.in_flight = true;
                self.bridge.send(Command::GetHistoryPage {
                    session_id: self.session_id.clone(),
                    before: Some(before),
                });
            }
            return;
        }
        if paging.in_flight || paging.remaining == 0 || paging.anchor.is_empty() {
            return;
        }
        paging.in_flight = true;
        self.bridge.send(Command::GetCompactedHistory {
            session_id: self.session_id.clone(),
            visible_messages: paging.visible.saturating_add(PAGE_SIZE),
        });
    }

    pub(crate) fn load_older_history(
        &mut self,
        result: Result<jcode_sdk::ApiEvent, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(paging) = &mut self.history_paging else {
            return;
        };
        paging.in_flight = false;
        let Ok(jcode_sdk::ApiEvent::CompactedHistory {
            messages,
            images,
            compacted_visible,
            compacted_remaining,
            ..
        }) = result
        else {
            // Leave the cursor unchanged so a later scroll can retry.
            return;
        };
        let (mut expanded, new_times) = Self::history_items(messages, images);
        let Some(prefix_len) = paging.prefix_len(&expanded) else {
            // A rewind, clear, or changed snapshot must never duplicate or
            // overwrite the live suffix. A later reconnect can resynchronise.
            return;
        };
        if compacted_visible <= paging.visible && compacted_remaining > 0 {
            return;
        }
        paging.visible = compacted_visible;
        paging.remaining = compacted_remaining;
        if prefix_len == 0 {
            return;
        }
        expanded.truncate(prefix_len);
        if let Some(offset) = paging.prompt_offset.as_mut() {
            *offset = offset.saturating_sub(
                expanded
                    .iter()
                    .filter(|item| super::prompt::is_user_turn(item))
                    .count(),
            );
        }
        if paging.remaining == 0 {
            paging.prompt_offset = Some(0);
        }
        paging.set_anchor(&expanded);
        self.prepend_history_items(expanded, new_times, cx);
    }

    pub(crate) fn load_bounded_history(
        &mut self,
        page: crate::harness::BoundedHistoryPage,
        cx: &mut Context<Self>,
    ) {
        // A reconnect supplies only the newest tail. Already loaded older rows
        // remain in the transcript, so their cursor and offset still describe
        // the beginning of the visible history, not this newer tail.
        let previous = self
            .history_loaded
            .then(|| self.history_paging.clone())
            .flatten();
        let prompt_offset = page
            .prompt_offset
            .or_else(|| page.before.is_none().then_some(0));
        self.load_history(page.messages, page.images, cx);
        if let Some(mut previous) = previous.filter(|paging| paging.bounded) {
            previous.in_flight = false;
            previous.recovery = None;
            self.history_paging = Some(previous);
            return;
        }
        self.history_paging = Some(HistoryPaging {
            visible: 0,
            remaining: 0,
            anchor: Vec::new(),
            bounded: true,
            cursor: page.before,
            prompt_offset,
            in_flight: false,
            recovery: None,
        });
    }

    pub(crate) fn load_bounded_history_page(
        &mut self,
        result: Result<crate::harness::BoundedHistoryPage, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(paging) = &mut self.history_paging else {
            return;
        };
        if !paging.bounded {
            return;
        }
        paging.in_flight = false;
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                eprintln!("jcode desktop: older history page failed: {error}");
                if error.contains("stale cursor") && paging.recovery.is_none() {
                    paging.set_anchor(&self.items);
                    if !paging.anchor.is_empty() {
                        paging.recovery = Some(HistoryRecovery::default());
                        paging.in_flight = true;
                        self.bridge.send(Command::GetHistoryPage {
                            session_id: self.session_id.clone(),
                            before: None,
                        });
                    }
                } else {
                    paging.recovery = None;
                }
                return;
            }
        };
        if let Some(mut recovery) = paging.recovery.take() {
            let (mut items, mut times) = Self::history_items(page.messages, page.images);
            let added = items.len();
            times.extend(
                recovery
                    .times
                    .drain()
                    .map(|(index, time)| (index + added, time)),
            );
            items.append(&mut recovery.items);
            if let Some(prefix) = paging.prefix_len(&items) {
                paging.cursor = page.before;
                if prefix > 0 {
                    items.truncate(prefix);
                    paging.prompt_offset = page
                        .prompt_offset
                        .or_else(|| paging.cursor.is_none().then_some(0));
                    paging.set_anchor(&items);
                    self.prepend_history_items(items, times, cx);
                } else if paging.cursor.is_some() {
                    self.request_older_history();
                }
            } else if let Some(before) = page.before {
                if recovery.requested_cursor.as_ref() == Some(&before) {
                    eprintln!("jcode desktop: older history recovery made no progress");
                    return;
                }
                recovery.requested_cursor = Some(before.clone());
                recovery.items = items;
                recovery.times = times;
                paging.recovery = Some(recovery);
                paging.in_flight = true;
                self.bridge.send(Command::GetHistoryPage {
                    session_id: self.session_id.clone(),
                    before: Some(before),
                });
            } else {
                eprintln!("jcode desktop: older history anchor no longer exists");
            }
            return;
        }
        let previous_cursor = paging.cursor.clone();
        paging.prompt_offset = page
            .prompt_offset
            .or_else(|| page.before.is_none().then_some(0));
        paging.cursor = page.before;
        let (items, times) = Self::history_items(page.messages, page.images);
        if items.is_empty() {
            if paging.cursor.is_some() && paging.cursor != previous_cursor {
                self.request_older_history();
            }
            return;
        }
        self.prepend_history_items(items, times, cx);
    }

    fn prepend_history_items(
        &mut self,
        mut expanded: Vec<Item>,
        new_times: std::collections::HashMap<usize, i64>,
        cx: &mut Context<Self>,
    ) {
        let prefix_len = expanded.len();
        if prefix_len == 0 {
            return;
        }
        let added_prompts = expanded
            .iter()
            .filter(|item| matches!(item, Item::User(_)))
            .count();
        for item in &mut self.items {
            if let Item::Image(image) = item
                && let Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal }) =
                    &mut image.anchor
            {
                *ordinal += added_prompts;
            }
        }
        let old_rows = self.transcript_render_rows().len();
        self.shift_user_times(prefix_len, 0);
        self.user_times
            .extend(new_times.into_iter().filter(|(i, _)| *i < prefix_len));
        for index in &mut self.pending_users {
            *index += prefix_len;
        }
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(i, at)| (i + prefix_len, at))
            .collect();
        self.expanded_prompts = std::mem::take(&mut self.expanded_prompts)
            .into_iter()
            .map(|(i, pinned)| (i + prefix_len, pinned))
            .collect();
        let prompt_scrolls = self.prompt_scrolls.get_mut();
        *prompt_scrolls = std::mem::take(prompt_scrolls)
            .into_iter()
            .map(|((i, pinned), scroll)| ((i + prefix_len, pinned), scroll))
            .collect();
        if let Some(index) = &mut self.image_pane_selected {
            *index += prefix_len;
        }
        for index in self.applet_positions.values_mut() {
            *index += prefix_len;
        }
        for (index, _) in &mut self.applet_rows {
            *index += prefix_len;
        }
        self.applet_rows_version = self.applet_rows_version.wrapping_add(1);
        expanded.append(&mut self.items);
        *self.items = expanded;
        self.transcript_measurements.dirty = true;
        let new_rows = self.transcript_render_rows().len();
        let added_rows = new_rows.saturating_sub(old_rows);
        // Inserting into ListState, rather than appending the count during
        // render, keeps the previously visible row at the same pixel offset.
        if self.transcript_row_count == old_rows + usize::from(self.activity_active()) {
            self.transcript_list.splice(0..0, added_rows);
            self.transcript_row_count += added_rows;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(values: &[(&str, &str)]) -> Vec<jcode_sdk::HistoryMessage> {
        values
            .iter()
            .map(|(role, content)| jcode_sdk::HistoryMessage {
                role: (*role).into(),
                content: (*content).into(),
                tool: None,
                timestamp_ms: None,
                response_stats: None,
            })
            .collect()
    }

    fn marker() -> &'static str {
        "Earlier conversation compacted - 2305 older historical messages hidden. Showing 68 of 2373 compacted messages. Scroll to the top to load more. Hidden user prompts: 145."
    }

    #[gpui::test]
    fn paged_history_numbers_use_absolute_prompt_offset(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.load_history(rows(&[("system", marker()), ("user", "new")]), vec![], cx)
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("prompt-number-0-146").is_some(),
            "initial tail has its absolute number"
        );
        let page: jcode_sdk::ApiEvent = serde_json::from_value(json!({"ev":"compacted_history", "session_id":"paging", "compacted_total":2373, "compacted_visible":192, "compacted_remaining":2304, "messages":[{"role":"user","content":"old"},{"role":"user","content":"new"}]})).unwrap();
        panel.update(vcx, |panel, cx| panel.load_older_history(Ok(page), cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-145").is_some());
        assert!(vcx.debug_bounds("prompt-number-1-146").is_some());
    }

    #[gpui::test]
    fn paged_decision_number_is_absolute(cx: &mut gpui::TestAppContext) {
        let ask: jcode_sdk::HistoryMessage = serde_json::from_value(json!({
            "role": "tool", "content": "User answers:\n1. Which? -> B\n",
            "tool": {"id": "ask-1", "name": "ask", "input": {"questions": [{"question": "Which?", "options": [{"label": "A"}, {"label": "B"}]}]}}
        })).unwrap();
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: vec![ask],
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: Some(5),
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("decision-number-6").is_some());
    }

    #[gpui::test]
    fn bounded_prompt_numbers_survive_prepend_and_reload(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "third")]),
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: Some(2),
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-3").is_some());
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "second")]),
                    images: vec![],
                    before: Some("earlier".into()),
                    prompt_offset: Some(1),
                }),
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-2").is_some());
        assert!(vcx.debug_bounds("prompt-number-1-3").is_some());
        panel.update(vcx, |panel, _| {
            let snapshot = panel.transcript_snapshot();
            panel.restore_transcript(snapshot);
        });
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "third")]),
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: Some(2),
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-1-3").is_some());
    }

    #[gpui::test]
    fn old_bounded_server_does_not_display_wrong_numbers(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "new")]),
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: None,
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-1").is_none());
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "old")]),
                    images: vec![],
                    before: None,
                    prompt_offset: None,
                }),
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-1").is_some());
        assert!(vcx.debug_bounds("prompt-number-1-2").is_some());
    }

    #[gpui::test]
    fn repeated_bounded_page_failures_do_not_spam_transcript(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "new")]),
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: Some(1),
                },
                cx,
            );
            for _ in 0..3 {
                panel.load_bounded_history_page(Err("network unavailable".into()), cx);
            }
            assert!(
                panel
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::Error(_)))
                    .count()
                    <= 1
            );
        });
    }

    #[gpui::test]
    fn stale_cursor_reanchors_without_duplicate_items(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "middle"), ("assistant", "answer"), ("user", "new")]),
                    images: vec![],
                    before: Some("stale".into()),
                    prompt_offset: Some(1),
                },
                cx,
            );
            panel.request_older_history();
        });
        let _ = commands.try_recv().unwrap();
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(Err("internal: stale cursor".into()), cx)
        });
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::GetHistoryPage { before: None, .. })
        ));
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[
                        ("user", "old"),
                        ("user", "middle"),
                        ("assistant", "answer"),
                        ("user", "new"),
                    ]),
                    images: vec![],
                    before: None,
                    prompt_offset: Some(0),
                }),
                cx,
            );
            assert_eq!(
                panel
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::User(text) if text == "middle"))
                    .count(),
                1
            );
            assert!(matches!(&panel.items[0], Item::User(text) if text == "old"));
        });
    }

    #[gpui::test]
    fn stale_cursor_reanchors_across_page_boundary(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "middle"), ("assistant", "answer"), ("user", "new")]),
                    images: vec![],
                    before: Some("stale".into()),
                    prompt_offset: Some(1),
                },
                cx,
            );
            panel.request_older_history();
            panel.load_bounded_history_page(Err("stale cursor".into()), cx);
        });
        let _ = commands.try_recv().unwrap();
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::GetHistoryPage { before: None, .. })
        ));
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[("assistant", "answer"), ("user", "new")]),
                    images: vec![],
                    before: Some("next".into()),
                    prompt_offset: Some(2),
                }),
                cx,
            );
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryPage { before: Some(before), .. }) if before == "next")
        );
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "old"), ("user", "middle")]),
                    images: vec![],
                    before: None,
                    prompt_offset: Some(0),
                }),
                cx,
            );
            assert_eq!(
                panel
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::User(text) if text == "middle"))
                    .count(),
                1
            );
            assert!(matches!(&panel.items[0], Item::User(text) if text == "old"));
        });
    }

    #[gpui::test]
    fn bounded_pages_prepend_and_continue_from_server_cursor(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "new")]),
                    images: vec![],
                    before: Some("cursor1".into()),
                    prompt_offset: Some(2),
                },
                cx,
            );
            panel.request_older_history();
            panel.request_older_history();
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryPage { before, .. }) if before.as_deref() == Some("cursor1"))
        );
        assert!(commands.try_recv().is_err());
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history_page(
                Ok(crate::harness::BoundedHistoryPage {
                    messages: rows(&[("user", "old")]),
                    images: vec![],
                    before: Some("cursor2".into()),
                    prompt_offset: Some(1),
                }),
                cx,
            );
            assert!(matches!(&panel.items[0], Item::User(text) if text == "old"));
            assert!(matches!(&panel.items[1], Item::User(text) if text == "new"));
            panel.request_older_history();
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryPage { before, .. }) if before.as_deref() == Some("cursor2"))
        );
    }

    #[gpui::test]
    fn bounded_history_requests_cursor_on_scroll_up(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(500.)));
        let messages = (0..50)
            .map(|i| jcode_sdk::HistoryMessage {
                role: "user".into(),
                content: format!("recent {i}"),
                timestamp_ms: None,
                response_stats: None,
                tool: None,
            })
            .collect();
        panel.update(vcx, |panel, cx| {
            panel.load_bounded_history(
                crate::harness::BoundedHistoryPage {
                    messages,
                    images: vec![],
                    before: Some("cursor".into()),
                    prompt_offset: Some(0),
                },
                cx,
            )
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = false;
            panel.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: 1,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        vcx.run_until_parked();
        let transcript = vcx
            .debug_bounds("transcript-with-response")
            .or_else(|| vcx.debug_bounds("transcript"))
            .unwrap();
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: transcript.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(300.))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryPage { before, .. }) if before.as_deref() == Some("cursor"))
        );
    }

    #[gpui::test]
    fn lazy_image_is_fetched_only_when_clicked(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        let image = TranscriptImage::new("image/png".into(), "jcode-lazy:abc".into(), None);
        panel.update(vcx, |panel, _| {
            *panel.items = vec![Item::Image(image.clone())];
        });
        assert!(commands.try_recv().is_err());
        vcx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.open_image_preview(image, window, cx))
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryImage { key, .. }) if key == "abc")
        );
        let data = image_preview::fixture_image().data;
        panel.update(vcx, |panel, cx| {
            panel.load_history_image("abc".into(), Ok(data), cx);
            assert!(panel.image_preview.is_some());
            assert!(matches!(&panel.items[0], Item::Image(image) if image.preview.is_some()));
        });
    }

    #[gpui::test]
    #[ignore = "requires isolated JCODE_HISTORY_API and generated session_bounded_fixture"]
    fn large_generated_session_through_sdk_and_panel(cx: &mut gpui::TestAppContext) {
        let socket = std::env::var("JCODE_HISTORY_API").unwrap();
        let id = "session_bounded_fixture";
        let client = jcode_sdk::JcodeClient::connect(jcode_sdk::ConnectOptions {
            socket_path: Some(socket.into()),
            client_name: "jcode-desktop-bounded-test".into(),
            request_timeout: Some(std::time::Duration::from_secs(120)),
            ensure_runtime: false,
            ..Default::default()
        })
        .unwrap();
        assert!(client.supports("bounded_history_v1"));
        client.attach_session(id).unwrap();
        let first = client.get_history_page(id, None).unwrap();
        let image = first
            .images
            .iter()
            .find(|image| image.data.starts_with("jcode-lazy:"))
            .unwrap();
        let key = image.data.strip_prefix("jcode-lazy:").unwrap();
        assert!(
            client
                .get_history_image(id, key)
                .unwrap()
                .starts_with("iVBORw0KGgo")
        );
        let cursor = first.before.clone().unwrap();
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new(id.into(), None, None, bridge, cx));
        panel.update(cx, |panel, cx| {
            panel.load_bounded_history(first, cx);
            assert!(panel.items.iter().any(
                |item| matches!(item, Item::User(text) if text == "synthetic user prompt 99")
            ));
            assert!(
                panel.items.iter().any(
                    |item| matches!(item, Item::Tool { name, .. } if name == "functions.swarm")
                )
            );
            panel.request_older_history();
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::GetHistoryPage { before, .. }) if before.as_deref() == Some(cursor.as_str()))
        );
        let second = client.get_history_page(id, Some(cursor)).unwrap();
        assert!(second.before.is_some());
        let old_image = second
            .images
            .iter()
            .find(|image| image.data.starts_with("jcode-lazy:"))
            .expect("older page image must remain lazy");
        assert!(
            client
                .get_history_image(id, old_image.data.strip_prefix("jcode-lazy:").unwrap())
                .unwrap()
                .starts_with("iVBORw0KGgo")
        );
        panel.update(cx, |panel, cx| {
            panel.load_bounded_history_page(Ok(second), cx);
            assert!(panel.items.iter().any(
                |item| matches!(item, Item::User(text) if text == "synthetic user prompt 30")
            ));
            assert!(panel.items.iter().any(
                |item| matches!(item, Item::User(text) if text == "synthetic user prompt 99")
            ));
        });
    }

    #[gpui::test]
    #[ignore = "requires JCODE_HISTORY_API for an isolated daemon with copied session"]
    fn copied_session_through_daemon_sdk_and_panel(cx: &mut gpui::TestAppContext) {
        let socket = std::env::var("JCODE_HISTORY_API").expect("set isolated JCODE_HISTORY_API");
        let id = "session_microbe_1794670947943_2648f3bbec7c7e5e";
        let client = jcode_sdk::JcodeClient::connect(jcode_sdk::ConnectOptions {
            socket_path: Some(socket.into()),
            request_timeout: Some(std::time::Duration::from_secs(120)),
            ensure_runtime: false,
            ..Default::default()
        })
        .unwrap();
        client.attach_session(id).unwrap();
        let (messages, images) = client.get_history_with_images(id).unwrap();
        let panel =
            cx.new(|cx| Panel::new(id.into(), None, None, crate::harness::spawn_inert(), cx));
        panel.update(cx, |panel, cx| panel.load_history(messages, images, cx));
        let full = client.get_compacted_history(id, usize::MAX).unwrap();
        let ApiEvent::CompactedHistory {
            messages,
            images,
            compacted_remaining,
            ..
        } = &full
        else {
            panic!("expected page")
        };
        assert_eq!(*compacted_remaining, 0);
        let (expected, _) = Panel::history_items(messages.clone(), images.clone());
        panel.update(cx, |panel, cx| panel.load_older_history(Ok(full), cx));
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.items.to_vec() == expected,
                "SDK history and panel differ"
            );
            assert!(
                panel
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::User(_)))
                    .count()
                    > 100
            );
        });
    }

    #[gpui::test]
    #[ignore = "requires JCODE_HISTORY_SCRATCH pointing at a copied session"]
    fn copied_session_windows_join_without_losing_prompts(cx: &mut gpui::TestAppContext) {
        let path = std::env::var("JCODE_HISTORY_SCRATCH")
            .expect("set JCODE_HISTORY_SCRATCH to a copied session");
        let session: jcode_base::session::Session =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let render = |window| {
            let (messages, images, info) =
                jcode_base::session::render_messages_and_images_with_compacted_history(
                    &session, window,
                );
            let messages: Vec<jcode_sdk::HistoryMessage> = messages
                .into_iter()
                .map(|m| {
                    serde_json::from_value(json!({
                        "role":m.role, "content":m.content, "timestamp_ms":m.timestamp_ms,
                        "response_stats":m.response_stats, "tool":m.tool_data
                    }))
                    .unwrap()
                })
                .collect();
            let images = serde_json::from_value(serde_json::to_value(images).unwrap()).unwrap();
            (messages, images, info.unwrap())
        };
        let panel = cx.new(|cx| {
            Panel::new(
                "copied".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let (messages, images, _) = render(64);
        panel.update(cx, |panel, cx| panel.load_history(messages, images, cx));
        let (full_messages, full_images, full_info) = render(usize::MAX);
        let (expected, _) = Panel::history_items(full_messages.clone(), full_images.clone());
        for window in [192, usize::MAX] {
            let (messages, images, info) = if window == usize::MAX {
                (
                    full_messages.clone(),
                    full_images.clone(),
                    full_info.clone(),
                )
            } else {
                render(window)
            };
            panel.update(cx, |panel, cx| {
                panel.load_older_history(
                    Ok(ApiEvent::CompactedHistory {
                        session_id: "copied".into(),
                        messages,
                        images,
                        compacted_total: info.total_messages,
                        compacted_visible: info.visible_messages,
                        compacted_remaining: info.remaining_messages,
                        compacted_hidden_prompts: info.hidden_user_prompts,
                    }),
                    cx,
                )
            });
        }
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.history_paging.as_ref().unwrap().remaining, 0);
            assert!(
                panel.items.to_vec() == expected,
                "paged and full transcripts differ"
            );
        });
    }

    #[gpui::test]
    fn scrolling_to_top_requests_page_and_keeps_anchor(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("paging".into(), None, None, bridge, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(500.)));
        let mut initial = rows(&[("system", marker())]);
        initial.extend((0..50).map(|i| jcode_sdk::HistoryMessage {
            role: "user".into(),
            content: format!("recent {i}"),
            timestamp_ms: None,
            response_stats: None,
            tool: None,
        }));
        panel.update(vcx, |panel, cx| {
            panel.load_history(initial.clone(), vec![], cx)
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = false;
            panel.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: 1,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        vcx.run_until_parked();
        let transcript = vcx
            .debug_bounds("transcript-with-response")
            .or_else(|| vcx.debug_bounds("transcript"))
            .unwrap();
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: transcript.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(300.))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::GetCompactedHistory {
                visible_messages: 192,
                ..
            })
        ));
        let before = panel.read_with(vcx, |panel, _| panel.transcript_list.logical_scroll_top());
        let mut expanded = rows(&[("system", marker()), ("user", "old 1"), ("user", "old 2")]);
        expanded.extend(initial.into_iter().skip(1));
        panel.update(vcx, |panel, cx| {
            panel.load_older_history(
                Ok(ApiEvent::CompactedHistory {
                    session_id: "paging".into(),
                    messages: expanded,
                    images: vec![],
                    compacted_total: 2373,
                    compacted_visible: 192,
                    compacted_remaining: 2181,
                    compacted_hidden_prompts: 100,
                }),
                cx,
            )
        });
        let after = panel.read_with(vcx, |panel, _| panel.transcript_list.logical_scroll_top());
        assert_eq!(after.item_ix, before.item_ix + 2);
        assert_eq!(after.offset_in_item, before.offset_in_item);
    }

    #[gpui::test]
    fn page_prepends_old_turns_without_replacing_live_suffix(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new("paging".into(), None, None, bridge, cx));
        panel.update(cx, |panel, cx| {
            panel.load_history(
                rows(&[
                    ("system", marker()),
                    ("user", "recent"),
                    ("assistant", "answer"),
                    ("compaction", "compacted"),
                    ("user", "current"),
                ]),
                vec![],
                cx,
            );
            panel.items.push(Item::User("live steer".into()));
            panel.request_older_history();
            panel.request_older_history();
        });
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::GetCompactedHistory {
                visible_messages: 192,
                ..
            })
        ));
        assert!(
            commands.try_recv().is_err(),
            "only one page may be in flight"
        );
        let page: ApiEvent = serde_json::from_value(json!({
            "ev":"compacted_history", "session_id":"paging", "compacted_total":2373,
            "compacted_visible":192, "compacted_remaining":2181,
            "messages":[{"role":"system","content":marker()},
                {"role":"user","content":"old"}, {"role":"assistant","content":"older answer"},
                {"role":"user","content":"recent"}, {"role":"assistant","content":"answer"},
                {"role":"compaction","content":"compacted"}, {"role":"user","content":"current"}]
        }))
        .unwrap();
        panel.update(cx, |panel, cx| panel.load_older_history(Ok(page), cx));
        panel.read_with(cx, |panel, _| {
            assert!(matches!(&panel.items[0], Item::User(text) if text == "old"));
            assert!(matches!(panel.items.last(), Some(Item::User(text)) if text == "live steer"));
            assert_eq!(
                panel
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::User(text) if text == "recent"))
                    .count(),
                1
            );
            assert!(
                panel
                    .items
                    .iter()
                    .position(|item| matches!(item, Item::User(text) if text == "old"))
                    < panel
                        .items
                        .iter()
                        .position(|item| matches!(item, Item::Compaction(_)))
            );
        });
        panel.update(cx, |panel, _| panel.request_older_history());
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::GetCompactedHistory {
                visible_messages: 320,
                ..
            })
        ));
    }

    #[gpui::test]
    fn stale_page_never_splices_unrelated_conversation(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| {
            Panel::new(
                "paging".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.load_history(rows(&[("system",marker()),("user","recent")]), vec![], cx);
            let page: ApiEvent = serde_json::from_value(json!({"ev":"compacted_history", "session_id":"paging", "compacted_total":2373, "compacted_visible":192, "compacted_remaining":2181, "messages":[{"role":"user","content":"unrelated"}]})).unwrap();
            panel.load_older_history(Ok(page), cx);
            assert_eq!(panel.items.to_vec(), vec![Item::User("recent".into())]);
            assert_eq!(panel.history_paging.as_ref().unwrap().visible, INITIAL_WINDOW);
        });
    }
}
