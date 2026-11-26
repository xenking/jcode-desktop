//! Painting a panel: transcript, composer dock and overlays.
use super::*;

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
        let _render_scope = crate::render_stats::scope("Panel");
        self.watch_composer_popover_focus(window, cx);
        if let Some(scope) = self.ask_focus_return.take()
            && (scope.contains_focused(window, cx) || self.focus_handle.is_focused(window))
        {
            self.focus_input(window, cx);
        }
        self.expandable_viewport_height = self.transcript_list.viewport_bounds().size.height;
        if std::mem::take(&mut self.focus_pending) {
            if self.composer_popover_focus(cx).is_none()
                && let Some(trigger) = self.composer_popover_trigger.take()
            {
                window.focus(&trigger, cx);
            } else {
                self.focus_input(window, cx);
            }
        }
        if std::mem::take(&mut self.image_preview_focus_pending)
            && self.image_preview.is_some()
            && self.owns_keyboard_focus(window, cx)
        {
            self.focus_handle.focus(window, cx);
        }
        if !self.tool_detail_motion.is_empty() {
            let now = Instant::now();
            let reduce_motion = cx.reduce_motion() || crate::config::get().appearance.reduce_motion;
            self.tool_detail_motion.retain(|_, motion| {
                motion.sample(now);
                !reduce_motion && motion.is_animating()
            });
            // Virtualized rows cache their geometry. Repaint during motion and
            // remeasure once more when the exiting card is finally removed.
            self.transcript_measurements.invalidate();
            if !self.tool_detail_motion.is_empty() {
                window.request_animation_frame();
            }
        }
        self.schedule_transcript_wheel_frame(window, cx);
        #[cfg(test)]
        crate::render_stats::test_renders::record(cx.entity_id());
        if self.is_side_document() {
            return self.render_side_document(window, cx);
        }
        if self.is_accounts_panel() {
            return div()
                .debug_selector(|| "accounts-panel".into())
                .size_full()
                .relative()
                .track_focus(&self.focus_handle)
                .key_context(crate::menu::CONTEXT)
                .on_action(cx.listener(|this, _: &crate::menu::Cancel, _, cx| {
                    this.close_login_picker(cx);
                    cx.emit(AccountsPanelClosed);
                }))
                .children(self.render_login_picker(window, cx))
                .into_any_element();
        }
        if self.is_change_review() {
            return div()
                .size_full()
                .relative()
                .track_focus(&self.focus_handle)
                .key_context(crate::menu::CONTEXT)
                .on_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                    this.close_diff_review(window, cx);
                }))
                .children(self.render_diff_review(cx))
                .into_any_element();
        }
        if self.applet.is_some() {
            return self.render_applet(window, cx);
        }
        if self.todoist.is_some() {
            return self.render_todoist(window, cx);
        }
        if self.openrig.is_some() {
            return self.render_openrig();
        }
        if self.orchestration.is_some() {
            return self.render_orchestration(window, cx);
        }
        if let Some(sessions) = &self.unfinished_work {
            let mut list = div()
                .id("unfinished-work-list")
                .debug_selector(|| "unfinished-work-list".into())
                .size_full()
                .overflow_y_scroll()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_size(px(18.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Unfinished work"))
                .child(div().text_size(px(11.)).text_color(Theme::global().TEXT_DIM).child("Todos left in closed sessions. Open the session from the sidebar to continue."));
            if sessions.is_empty() {
                list = list.child(
                    div()
                        .mt_4()
                        .text_color(Theme::global().TEXT_DIM)
                        .child("You are all caught up."),
                );
            }
            for (index, session) in sessions.iter().enumerate() {
                let session_to_open = session.clone();
                let open_session = self.unfinished_session_opener.clone();
                let mut card = div()
                    .id(("unfinished-session", index))
                    .debug_selector(move || format!("unfinished-session-{index}"))
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(Theme::global().PANEL_BORDER)
                    .bg(Theme::global().HEADER_BG)
                    .cursor_pointer()
                    .hover(|card| {
                        card.border_color(Theme::global().PANEL_BORDER_FOCUS)
                            .bg(Theme::global().ACCENT_DIM)
                    })
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        // Routing into the workspace can read or focus any panel,
                        // including this one. Do not hold a Panel update lease via
                        // cx.listener while invoking the opener.
                        move |_event, window, cx| {
                            if let Some(open_session) = &open_session {
                                open_session(session_to_open.clone(), window, cx);
                                cx.stop_propagation();
                            }
                        },
                    )
                    .on_mouse_up(gpui::MouseButton::Left, move |_event, _window, cx| {
                        cx.stop_propagation();
                    })
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(session.title.clone()),
                    );
                if let Some(dir) = &session.working_dir {
                    card = card.child(
                        div()
                            .text_size(px(10.))
                            .text_color(Theme::global().TEXT_FAINT)
                            .child(dir.clone()),
                    );
                }
                for todo in &session.todos {
                    let marker = if todo.status.eq_ignore_ascii_case("in_progress") {
                        "◐"
                    } else {
                        "○"
                    };
                    card = card.child(
                        div()
                            .flex()
                            .gap_2()
                            .text_size(px(12.))
                            .child(marker)
                            .child(todo.content.clone()),
                    );
                }
                list = list.child(card);
            }
            return div()
                .size_full()
                .track_focus(&self.focus_handle)
                .child(list)
                .into_any_element();
        }
        let streaming = !self.streaming_text.is_empty();
        let transcript_selection = self.transcript_selection.clone();
        let transcript_shell = div()
            .relative()
            // Keyed descendants (including browser-backed image previews) must
            // retain their entities when an unrelated streamed reply starts or
            // ends. Only the debug selector reflects streaming state.
            .id("transcript")
            .children(crate::fixture_control_targets::marker(
                || "history:scroll-up".into(),
                || Some(self.session_id.clone()),
            ))
            .children(
                (crate::fixture_control_targets::transcript_scroll_fixture()
                    || crate::fixture_control_targets::context_overlay_fixture())
                .then(|| {
                    crate::fixture_control_targets::marker(
                        || "scroll:transcript".into(),
                        || Some(self.session_id.clone()),
                    )
                    .unwrap()
                    .right_auto()
                    .w(px(4.))
                }),
            )
            .children(crate::fixture_control_targets::transcript_scroll_fixture().then(|| {
                crate::fixture_control_targets::marker(
                    || "scroll:pin-prompt".into(),
                    || Some(self.session_id.clone()),
                ).unwrap().right_auto().w(px(4.))
            }))
            // Tagged so render tests can assert a streamed response painted.
            .debug_selector(move || {
                if streaming {
                    "transcript-with-response".into()
                } else {
                    "transcript".into()
                }
            })
            .on_mouse_up(gpui::MouseButton::Left, move |_event, _window, cx| {
                transcript_selection.update(cx, |selection, cx| {
                    selection.finish();
                    cx.notify();
                });
            })
            .size_full()
            .text_size(px(13.5))
            .overflow_hidden()
            // Clear and collect only this frame's painted leaf geometry before
            // the virtual list paints. Logical text is maintained independently.
            .child(text_selection::surface(
                self.transcript_selection.clone(),
                Some(self.transcript_list.clone()),
            ));
        let transcript_shell =
            TextSelection::focus_scope(transcript_shell, &self.transcript_selection, cx);

        // Live rows are appended after the settled ones and share the same
        // renderer, so a streaming turn looks identical to a finished one.
        // Todo state is persistent session chrome rather than transcript history.
        // Keep only the latest snapshot pinned above the scroller instead of
        // leaving stale cards interspersed through the conversation.
        let instant_motion = self.tick_stream_reveal(window, cx);
        self.sync_applet_rows(cx);
        let clean_chat = self.clean_chat_enabled();
        let previous_projection = self
            .transcript_derived
            .as_ref()
            .filter(|derived| derived.key.clean_chat != clean_chat)
            .map(|derived| derived.rows.clone());
        let derived = self.transcript_derived(cx);
        let rows = derived.rows.clone();
        let display = derived.key.display.clone();
        let prompt_rows = derived.prompt_rows.clone();
        let latest_todo = (!clean_chat).then(|| derived.latest_todo.clone()).flatten();
        let tail_headers = if clean_chat {
            [0; 2]
        } else {
            derived.tail_headers
        };
        let active_error = derived.active_error;
        let projection_changed = previous_projection.is_some();
        if let Some(previous) = &previous_projection {
            self.remap_rewind_projection(previous, &rows);
        }
        let anchor = previous_projection
            .filter(|_| !self.stick_to_bottom)
            .and_then(|previous| {
                clean_chat::remap_anchor(
                    &previous,
                    &rows,
                    self.transcript_list.logical_scroll_top(),
                )
            });
        if projection_changed {
            self.transcript_measurements.invalidate();
            self.cancel_transcript_momentum();
            self.tail_glide.reset();
        }
        // The publish tracker replaces the collapsible todo summary with
        // fixed, always-visible pipeline stages fed by the same snapshot.
        let publish_tracker = if clean_chat {
            None
        } else {
            self.render_publish_tracker(latest_todo.as_deref())
        };
        let pinned_todo =
            latest_todo.filter(|payload| !payload.todos.is_empty() && publish_tracker.is_none());
        let has_pinned_todo = pinned_todo.is_some() || publish_tracker.is_some();
        // Pinned chrome resizes the transcript viewport when it changes.
        let tail_header = tail_headers[usize::from(publish_tracker.is_some())];
        // A separate virtual row keeps activity below text, reasoning, and tools.
        // Only the small Spinner entity ticks, never the transcript itself.
        let show_turn_activity = self.shows_transcript_activity();
        let row_count = rows.len() + usize::from(show_turn_activity);
        // Derive the empty state from session content, not the draft. Typing,
        // pasting attachments, and reconnecting must not move the composer.
        let fresh_session = !self.transcript_only
            && !self.awaiting_persisted
            && self.items.is_empty()
            && row_count == 0
            && !self.activity_active()
            && !self
                .startup_layout
                .as_ref()
                .is_some_and(|layout| layout.committed);
        if !fresh_session
            && !self.items.is_empty()
            && let Some(layout) = &mut self.startup_layout
        {
            layout.committed = true;
        }
        // The ordinary composer floats over the bottom of the transcript, so
        // earlier history scrolls beneath its translucent veil instead of
        // being cut off at a hard edge. The list pads its end by the dock.
        let docked_composer =
            !fresh_session && self.startup_layout.is_none() && !self.transcript_only;
        let dock_pad = if docked_composer {
            self.composer_dock_height + TRANSCRIPT_BOTTOM_GAP
        } else {
            0.0
        };
        let transcript_shell =
            transcript_shell.when(!docked_composer, |el| el.pb(px(TRANSCRIPT_BOTTOM_GAP)));
        if self
            .input
            .read(cx)
            .needs_layout_update(false, composer::VOICE_TRAILING_SPACE)
        {
            self.input.update(cx, |input, cx| {
                // The composer keeps one compact docked size everywhere,
                // including the welcome screen, so it never changes shape
                // when the conversation starts.
                input.set_spacious(false, cx);
                input.set_trailing_inset(composer::VOICE_TRAILING_SPACE, cx);
            });
        }
        if self.input.read(cx).working_dir() != self.working_dir.as_deref() {
            let working_dir = self.working_dir.clone();
            self.input.update(cx, |input, _| {
                input.set_working_dir(working_dir.as_deref());
            });
        }
        let context_root = self.composer_context_root();
        if self.input.read(cx).context_root() != context_root {
            self.input.update(cx, |input, cx| input.set_context_root(context_root, cx));
        }
        if let Some((PickerKind::Context, search)) = &self.picker_search
            && search.read(cx).context_root() != context_root
        {
            search.update(cx, |input, cx| input.set_context_root(context_root, cx));
        }
        let row_count_changed = row_count != self.transcript_row_count;
        if row_count_changed {
            if row_count > self.transcript_row_count {
                self.transcript_list.splice(
                    self.transcript_row_count..self.transcript_row_count,
                    row_count - self.transcript_row_count,
                );
            } else {
                // Streaming/activity rows can disappear or merge when a chunk
                // settles. Resetting discards the reader's logical position.
                // Trim only the tail so the visible history stays anchored.
                self.transcript_list
                    .splice(row_count..self.transcript_row_count, 0);
            }
            self.transcript_row_count = row_count;
        }
        let theme = Theme::global();
        if let Some(range) = self.transcript_measurements.take_range(
            self.items.len(),
            row_count,
            (
                self.reasoning_reveal
                    .visible(&self.streaming_reasoning)
                    .len()
                    + self.reasoning_reveal.fading(&self.streaming_reasoning),
                self.text_reveal.visible(&self.streaming_text).len()
                    + self.text_reveal.fading(&self.streaming_text),
            ),
            (theme.FONT_UI, theme.FONT_AI, theme.FONT_MONO),
        ) {
            self.transcript_list.remeasure_items(range);
        }
        if let Some(anchor) = anchor {
            self.transcript_list.scroll_to(anchor);
        }
        if (self.transcript_end_pad - dock_pad).abs() > 0.5 {
            self.transcript_end_pad = dock_pad;
            if row_count > 0 {
                self.transcript_list
                    .remeasure_items(row_count - 1..row_count);
            }
        }
        if row_count > 0
            && let Some((_, y)) = self.pending_history_scroll.take()
        {
            // Restore relative to the first row, not the scrollbar's
            // partially measured total. The virtual list measures and
            // clamps this logical position while laying out the history.
            self.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px((-y).max(0.0)),
            });
            // Scrollbar geometry is available after this layout pass.
            window.request_animation_frame();
        }
        let startup_preview = self.startup_prompt_preview();
        // While the welcome viewport still grows with its content, following
        // the tail would scroll a line up for one frame and then snap back
        // when the composer moves down. Hold the top until the space fills.
        if self.rewind_selection.is_some() {
            self.tail_glide.reset();
        } else if startup_preview || self.startup_growing() {
            self.tail_glide.reset();
            self.transcript_list.scroll_to(gpui::ListOffset::default());
        } else if self.stick_to_bottom {
            // Ease wrapped-line growth of live text instead of jumping a line.
            self.follow_transcript_tail(
                instant_motion,
                tail_header,
                cx.background_executor().now(),
                window,
            );
        } else {
            self.tail_glide.reset();
        }
        let rewind_inset = self.reveal_rewind_selection(&rows, dock_pad, clean_chat, window, cx);

        let input_bounds = std::rc::Rc::new(std::cell::Cell::new(None));
        let body_bounds = std::rc::Rc::new(std::cell::Cell::new(None));
        let panel = cx.entity();
        let wheel_panel = panel.clone();
        let wheel_input_bounds = input_bounds.clone();
        let (wheel_root, wheel_hitbox) = crate::transcript_scroll::capture_root();
        let list_rows = rows.clone();
        let first_visible_row = std::rc::Rc::new(std::cell::Cell::new(None));
        let row_first_visible = first_visible_row.clone();
        let prompt_list = self.transcript_list.clone();
        let end_visible = std::rc::Rc::new(std::cell::Cell::new(false));
        let row_end_visible = end_visible.clone();
        let end_list = self.transcript_list.clone();
        let steady_cell = self.steady_activity.clone();
        // Smooth the spinner only while it rides the gliding tail. Manual
        // scrolling must move it exactly with the content.
        let steady_enabled = self.stick_to_bottom
            && !instant_motion
            && !crate::config::get().appearance.reduce_motion;
        let short_viewport = window.viewport_size().height < px(400.);
        let transcript = if fresh_session {
            div()
                .debug_selector(|| "fresh-session".into())
                .size_full()
                .flex()
                // Short windows anchor the composer (and its pill row below)
                // to the bottom so it never spills into the footer.
                .map(|el| {
                    if short_viewport {
                        el.items_end()
                    } else {
                        el.items_center()
                    }
                })
                .justify_center()
                .px_4()
                .pb(px(if short_viewport { 8. } else { 64. }))
                .child(
                    div()
                        .w_full()
                        .max_w(px(1120.0))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .text_size(px(22.0))
                                .text_color(Theme::global().TEXT)
                                .child("What would you like to work on?"),
                        )
                        .child(self.render_composer_stack(input_bounds.clone(), false, window, cx)),
                )
                .into_any_element()
        } else if row_count == 0 {
            transcript_shell.into_any_element()
        } else {
            // Capture before List mutably borrows its state during row rendering.
            let response_viewport_height = (self.transcript_list.viewport_bounds().size.height
                - px(dock_pad + prompt::PROMPT_TOP_PADDING))
            .max(px(1.));
            let file_links = self
                .working_dir
                .as_deref()
                .filter(|_| !crate::harness::is_remote_session(&self.session_id))
                .map(|base| markdown::FileLinkHandler::for_transcript(PathBuf::from(base)));
            transcript_shell
                .child(
                    list(
                        self.transcript_list.clone(),
                        move |row_index, window, cx| {
                            // The last row carries the floating composer's
                            // height as padding, so the live end scrolls clear
                            // of it while history passes beneath its veil.
                            let end_pad = if row_index + 1 == row_count {
                                dock_pad
                            } else {
                                0.0
                            };
                            if row_index == list_rows.len() {
                                return div()
                                    .relative()
                                    .pb(px(end_pad) + rewind_inset)
                                    .child(steady_activity::steady_row(
                                        panel.read(cx).render_transcript_activity(),
                                        steady_cell.clone(),
                                        steady_enabled,
                                        end_pad,
                                    ))
                                    .child(latest::end_marker(
                                        row_end_visible.clone(),
                                        end_list.clone(),
                                    ))
                                    .into_any_element();
                            }
                            let row = &list_rows[row_index];
                            panel.update(cx, |panel, cx| {
                                let item = match &row.source {
                                    TranscriptRowSource::Settled(index) => &panel.items[*index],
                                    TranscriptRowSource::Owned(item) => item,
                                };
                                // Keep message boundaries airy, but pack consecutive tool calls.
                                let follows_tool =
                                    row_index.checked_sub(1).is_some_and(|previous| {
                                        let previous = match &list_rows[previous].source {
                                            TranscriptRowSource::Settled(index) => {
                                                &panel.items[*index]
                                            }
                                            TranscriptRowSource::Owned(item) => item,
                                        };
                                        matches!(previous, Item::Tool { .. })
                                    });
                                let top_padding = if display.compact_notifications
                                    && matches!(item, Item::Stopped(_) | Item::CacheMiss(_))
                                    || matches!(item, Item::ResponseStats(_) | Item::AgentMessage { .. } | Item::SideQuestion(_))
                                    || matches!(item, Item::Tool { .. }) && follows_tool
                                {
                                    2.0
                                } else {
                                    prompt::PROMPT_TOP_PADDING
                                };
                                let mut terms = panel.search_terms.clone();
                                let highlighted = row.index < panel.items.len()
                                    && panel.control_highlight().is_some_and(|reveal| reveal.range.contains(&row.index));
                                if highlighted && let Some(text) = panel.control_highlight().and_then(|reveal| reveal.text.as_ref()) {
                                    terms.push(text.clone());
                                }
                                let mut render_item = || markdown::with_search_terms(&terms, || {
                                    panel.render_item(
                                        row.index,
                                        item,
                                        &display,
                                        row.show_label,
                                        response_viewport_height,
                                        window,
                                        cx,
                                    )
                                });
                                let element = match &file_links {
                                    Some(handler) => {
                                        let infer_references = matches!(item,
                                            Item::Assistant(_) | Item::AssistantOutput { .. } | Item::AgentMessage { .. }
                                        ) || matches!(item, Item::Tool { name, .. } if name == "user_response");
                                        markdown::with_file_link_handler(handler.clone(), infer_references, render_item)
                                    }
                                    _ => render_item(),
                                };
                                let heading = clean_chat.then(|| {
                                    let previous = row_index.checked_sub(1).map(|index| match &list_rows[index].source {
                                        TranscriptRowSource::Settled(index) => &panel.items[*index],
                                        TranscriptRowSource::Owned(item) => item,
                                    });
                                    clean_chat::message_heading(item, previous)
                                }).flatten();
                                let padded = div()
                                    .debug_selector(move || format!("transcript-row-{row_index}"))
                                    .relative()
                                    .when(highlighted, |el| el.rounded_lg().bg(Theme::global().wash(Ink::Accent, 0.14)))
                                    .px_3()
                                    .pt(px(top_padding))
                                    .map(|el| match display.alignment {
                                        crate::config::TextAlignment::Left => el.text_left(),
                                        crate::config::TextAlignment::Center => el.text_center(),
                                        crate::config::TextAlignment::Right => el.text_right(),
                                    })
                                    .when(display.debug_visual, |el| {
                                        el.border_1().border_color(Theme::global().TOOL_BORDER)
                                    })
                                    .children(heading)
                                    .child(element)
                                    .when(
                                        display.tool_call_details && !display.screenshot_mode,
                                        |el| {
                                            if let Item::Tool {
                                                call_id,
                                                input,
                                                done,
                                                ..
                                            } = item
                                            {
                                                el.child(
                                                    div()
                                                        .px_1()
                                                        .text_size(px(10.))
                                                        .text_color(Theme::global().TEXT_FAINT)
                                                        .child(format!(
                                                            "call {call_id} · {} · {input}",
                                                            if *done {
                                                                "finished"
                                                            } else {
                                                                "running"
                                                            }
                                                        )),
                                                )
                                            } else {
                                                el
                                            }
                                        },
                                    )
                                    .child(prompt::visibility_marker(
                                        row_index,
                                        classify::is_pinnable_prompt(item),
                                        row_first_visible.clone(),
                                        prompt_list.clone(),
                                    ));
                                let rendered = if row.index < panel.items.len() {
                                    panel.render_rewind_row(row.index, item, padded, cx)
                                } else {
                                    padded.into_any_element()
                                };
                                let last = row_index + 1 == row_count;
                                if last || (row_index == 0 && rewind_inset > px(0.)) {
                                    div()
                                        .relative()
                                        .pt(if row_index == 0 { rewind_inset } else { px(0.) })
                                        .pb(if last {
                                            rewind_inset + px(end_pad)
                                        } else {
                                            px(0.)
                                        })
                                        .child(rendered)
                                        .when(last, |el| {
                                            el.child(latest::end_marker(
                                                row_end_visible.clone(),
                                                end_list.clone(),
                                            ))
                                        })
                                        .into_any_element()
                                } else {
                                    rendered
                                }
                            })
                        },
                    )
                    .size_full()
                    .max_w(px(1120.))
                    .mx_auto(),
                )
                .into_any_element()
        };

        // Keep the same spacious editor at its measured welcome position.
        // The transcript spends the blank space above it before the editor
        // moves down. Blank space belongs to layout, never transcript rows.
        let transcript = if !fresh_session {
            if let Some(layout) = &self.startup_layout {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(layout.messages_height))
                            .min_h_0()
                            .flex_shrink_1()
                            .child(transcript),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex_none()
                            .px_4()
                            .pb(px(COMPOSER_BOTTOM_INSET))
                            .flex()
                            .justify_center()
                            .child(self.render_composer_stack(input_bounds.clone(), false, window, cx)),
                    )
                    .into_any_element()
            } else {
                transcript
            }
        } else {
            transcript
        };

        let theme = Theme::global();
        // The tail may sit a few pixels below the fold mid-glide. While the
        // welcome layout is still growing, the end is only clipped for the
        // frame before the composer moves down.
        let startup_growing = self.startup_growing();
        let show_jump_chip = row_count > 0
            && !self.transcript_end_visible
            && !self.tail_gliding()
            && !startup_growing;

        let chat = div()
            .flex()
            .flex_col()
            .size_full()
            .when(self.image_pane_open, |el| {
                el.flex_1()
                    .min_w_0()
                    .min_h_0()
                    .when(self.image_pane_stacked, |el| el.h_auto())
            })
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context(self.key_context())
            .on_action(cx.listener(Self::menu_cancel))
            .on_action(cx.listener(Self::menu_confirm))
            .on_action(cx.listener(Self::menu_select_next))
            .on_action(cx.listener(Self::menu_select_previous))
            .capture_action(cx.listener(|this, _: &crate::input::Clear, window, cx| {
                if this.finish_rewind(false, window, cx) {
                    cx.stop_propagation();
                } else if this.queue_editor_focused(window, cx) {
                    // Esc first closes an attachment preview the editor opened.
                    if this.queue_editor_has_preview(cx) {
                        cx.propagate();
                    } else {
                        this.cancel_queued_prompt_edit(window, cx);
                        cx.stop_propagation();
                    }
                } else if this.ask_editor_focused(window, cx) {
                    window.focus(&this.focus_handle, cx);
                    cx.notify();
                    cx.stop_propagation();
                } else {
                    cx.propagate();
                }
            }))
            // Up in an empty composer opens the newest queued message for
            // editing in its own queue row. Anything the queue does not claim
            // falls through to the composer's ordinary prompt history.
            .capture_action(
                cx.listener(|this, _: &crate::input::HistoryPrev, window, cx| {
                    if this.queue_recall_prev(window, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .capture_action(cx.listener(|this, _: &crate::input::Submit, window, cx| {
                if this.queue_editor_focused(window, cx) {
                    this.save_queued_prompt_edit(window, cx);
                } else if !this.handle_ask_editor_submit(window, cx) {
                    cx.propagate();
                    return;
                }
                cx.stop_propagation();
            }))
            // Files dragged in from the file manager: images attach like a
            // paste, other files are referenced by path in the prompt.
            .when(!self.transcript_only && !self.demo, |el| {
                el.drag_over::<gpui::ExternalPaths>(|style, _, _, _| {
                    style.bg(Theme::global().SELECTION)
                })
                .on_drop(cx.listener(
                    |panel, paths: &gpui::ExternalPaths, window, cx| {
                        // The input's change observer updates this panel, so
                        // apply the drop after this panel update returns.
                        let paths = paths.paths().to_vec();
                        let input = panel.input.clone();
                        window.defer(cx, move |window, cx| {
                            input.update(cx, |input, cx| input.drop_files(&paths, window, cx));
                        });
                    },
                ))
            })
            .on_action(cx.listener(|panel, _: &shortcuts::JumpToLatest, _, cx| {
                panel.jump_to_latest(cx);
            }))
            .on_action(cx.listener(|panel, _: &shortcuts::ToggleCleanChat, _, cx| {
                panel.toggle_clean_chat(cx);
            }))
            .on_action(
                cx.listener(|panel, _: &shortcuts::BackgroundRunningTool, _, cx| {
                    panel.background_running_tool_if_any(cx);
                }),
            )
            .on_action(
                cx.listener(|panel, _: &shortcuts::JumpToLatestIfEmpty, window, cx| {
                    panel.jump_to_latest_if_empty(window, cx);
                }),
            )
            .on_action(cx.listener(|panel, _: &voice::ToggleVoice, window, cx| {
                if !panel.voice_active() {
                    panel.capture_focused_ask_voice_target(window, cx);
                }
                panel.toggle_voice(cx);
            }))
            .on_action(
                cx.listener(|panel, _: &crate::effort::IncreaseEffort, _, cx| {
                    panel.step_effort(1, cx);
                }),
            )
            .on_action(
                cx.listener(|panel, _: &crate::effort::DecreaseEffort, _, cx| {
                    panel.step_effort(-1, cx);
                }),
            )
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.handle_rewind_key(event, cx)
                    || this.handle_queue_card_key(event, cx)
                    || this.handle_ask_key(event, window, cx)
                    || this.handle_routing_gate_key(event, window, cx)
                    || this.resume_composer_typing(event, false, window, cx)
                {
                    cx.stop_propagation();
                }
            }))
            .children(crate::harness::remote_host(&self.session_id).map(|host| {
                let machine = match crate::managed_cloud::Destination::parse(&host) {
                    Some(target) => target
                        .name()
                        .map_or_else(|| "Jcode Cloud".into(), |name| format!("Cloud · {name}")),
                    None => format!("SSH · {host}"),
                };
                let label = match self.working_dir.as_deref().filter(|d| !d.is_empty()) {
                    Some(dir) => format!("Running on {machine} · {dir}"),
                    None => format!("Running on {machine}"),
                };
                div()
                    .debug_selector(|| "panel-remote-host".into())
                    .flex_none()
                    .px_3()
                    .py_1()
                    .text_size(px(11.0))
                    .text_color(theme.ACCENT)
                    .bg(theme.ACCENT_DIM)
                    .truncate()
                    .child(label)
            }))
            .children(self.render_chat_blockers(active_error, window, cx))
            .children(publish_tracker)
            .children(self.render_rewind_hint())
            .children(pinned_todo.map(|payload| {
                div()
                    .id("pinned-todo-scroll")
                    .debug_selector(|| "pinned-todo-card".into())
                    .flex_none()
                    .px_3()
                    .pt_1()
                    .mb_2()
                    .max_h(px(
                        (f32::from(window.viewport_size().height) * 0.3).min(280.)
                    ))
                    .overflow_y_scroll()
                    .child(render_todo_card_with_style(
                        &payload,
                        true,
                        "pinned-todo",
                        &self.transcript_selection,
                        window,
                        cx,
                    ))
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when(!has_pinned_todo, |body| body.mt_2())
                    .child(wheel_root)
                    .child(transcript)
                    .children(self.render_pinned_prompt(cx))
                    // Pace both wheel and touchpad movement on display frames.
                    // Precise deltas use a much shorter smoothing window so
                    // native gesture control stays responsive.
                    .child(
                        gpui::canvas(
                            |_, _, _| (),
                            move |_, _, window, _| {
                                let wheel_panel = wheel_panel.clone();
                                let input_bounds = wheel_input_bounds.clone();
                                let hitbox = wheel_hitbox.clone();
                                window.on_mouse_event(
                                    move |event: &gpui::ScrollWheelEvent, phase, window, cx| {
                                        if phase != gpui::DispatchPhase::Capture {
                                            return;
                                        }
                                        let child_owns_wheel =
                                            crate::transcript_scroll::end_capture(cx);
                                        if !hitbox.get().is_some_and(|hitbox| {
                                            hitbox.should_handle_scroll(window)
                                        }) {
                                            return;
                                        }
                                        let in_composer = input_bounds.get().is_some_and(
                                            |bounds: gpui::Bounds<gpui::Pixels>| {
                                                bounds.contains(&event.position)
                                            },
                                        );
                                        let (over_editor, editor_gesture) = wheel_panel
                                            .read(cx)
                                            .input
                                            .read(cx)
                                            .editor_wheel_ownership(event, in_composer);
                                        if editor_gesture && !over_editor {
                                            cx.stop_propagation();
                                            return;
                                        }
                                        if child_owns_wheel || over_editor {
                                            if wheel_panel.read(cx).transcript_wheel_frame.is_some()
                                            {
                                                wheel_panel.update(cx, |panel, _| {
                                                    panel.cancel_transcript_momentum()
                                                });
                                            }
                                            return;
                                        }
                                        if in_composer {
                                            return;
                                        }
                                        let delta = event.delta.pixel_delta(window.line_height());
                                        let y = f32::from(delta.y);
                                        if y == 0.0 {
                                            return;
                                        }
                                        let panel = wheel_panel.read(cx);
                                        if panel.transcript_list.is_scrollbar_dragging() {
                                            cx.stop_propagation();
                                            return;
                                        }
                                        if panel.diff_review.is_some() {
                                            return;
                                        }
                                        if panel.startup_layout.is_some()
                                            && !panel
                                                .transcript_list
                                                .viewport_bounds()
                                                .contains(&event.position)
                                        {
                                            return;
                                        }
                                        let precise = event.delta.precise();
                                        wheel_panel.update(cx, |panel, cx| {
                                            if precise {
                                                panel.glide_transcript_input(-y, true, cx);
                                            } else {
                                                panel.glide_transcript_wheel(-y, cx);
                                            }
                                        });
                                        cx.stop_propagation();
                                    },
                                );
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .when(docked_composer, |el| {
                        el.child(self.render_composer_dock(input_bounds.clone(), window, cx))
                    })
                    .child(startup::input_marker(body_bounds.clone()))
                    .child(self.prompt_visibility_observer(prompt_rows, first_visible_row, cx))
                    .child(self.transcript_end_observer(end_visible, row_count, cx))
                    .child(self.tail_growth_observer(cx))
                    .when(!startup_growing, |el| {
                        el.child(crate::scrollbar::interactive_vertical_list_with_inset(
                            &self.transcript_list,
                            "transcript-scrollbar",
                            dock_pad,
                            {
                                let panel = cx.entity().downgrade();
                                move |_, cx| {
                                    let _ = panel.update(cx, |panel, cx| {
                                        panel.cancel_transcript_momentum();
                                        panel.release_startup_preview();
                                        panel.stick_to_bottom = false;
                                        cx.notify();
                                    });
                                }
                            },
                            {
                                let panel = cx.entity().downgrade();
                                move |_, cx| {
                                    let _ = panel.update(cx, |panel, cx| {
                                        panel.resume_transcript_follow_after_scroll(cx);
                                        cx.notify();
                                    });
                                }
                            },
                        ))
                    })
                    // Float catch-up activity above the composer, centered in the
                    // transcript rather than tucked into the scrollbar corner.
                    .when(show_jump_chip, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "latest-pill-anchor".into())
                                .absolute()
                                .map(|el| {
                                    if self.startup_layout.is_some() {
                                        let height = f32::from(
                                            self.transcript_list.viewport_bounds().size.height,
                                        );
                                        el.top(px((height - 32.).max(0.)))
                                    } else {
                                        el.bottom(px(dock_pad + 8.))
                                    }
                                })
                                .left_3()
                                .right_3()
                                .flex()
                                .justify_center()
                                .child(
                                    div()
                                        .id("jump-to-latest")
                                        .debug_selector(|| "jump-to-latest".into())
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_full()
                                        .bg(Theme::global().HEADER_BG)
                                        .border_1()
                                        .border_color(Theme::global().PANEL_BORDER)
                                        .text_size(px(10.5))
                                        .font_family(Theme::global().FONT_MONO)
                                        .text_color(Theme::global().TEXT_DIM)
                                        .cursor_pointer()
                                        .hover(|el| el.text_color(Theme::global().TEXT))
                                        .occlude()
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(|this, _event, _window, cx| {
                                                this.jump_to_latest(cx);
                                            }),
                                        )
                                        .when(show_turn_activity, |el| {
                                            el.child(
                                                div()
                                                    .debug_selector(|| "latest-activity".into())
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_color(Theme::global().ACCENT)
                                                    .child(self.latest_activity_spinner.clone()),
                                            )
                                        })
                                        .child("↓")
                                        .child(shortcuts::jump_to_latest_keycaps(
                                            crate::theme::to_hsla(Theme::global().TEXT_DIM),
                                        )),
                                ),
                        )
                    }),
            )
            .when(self.recovery_picker_open && !docked_composer, |el| {
                el.child(self.render_recovery_model_picker(cx))
            })
            .when(!docked_composer, |el| {
                el.children(self.render_voice_overlay(window, cx))
                    .children(self.render_preview_badge(cx))
            })
            // The docked composer floats over the transcript body above.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    // A read-only preview must not steal focus from its host.
                    if !this.transcript_only {
                        window.focus(&this.transcript_selection.read(cx).focus_handle(), cx);
                    }
                    cx.notify();
                }),
            )
            .child(self.startup_layout_observer(
                fresh_session,
                input_bounds.clone(),
                body_bounds,
                cx,
            ))
            .child(flicker::observer(
                self.flicker_diagnostics.clone(),
                self.transcript_list.clone(),
                input_bounds,
                self.offscreen_prompt.is_some(),
                cx.entity().entity_id(),
            ))
            .child(self.input.read(cx).paste_preview_panel_marker())
            .children(self.render_image_preview(window, cx))
            .children(self.render_diff_review(cx))
            .children(self.render_login_picker(window, cx))
            .into_any_element();

        // Inline mode retains the existing chat layout and event ancestry.
        if !self.image_pane_open {
            return chat;
        }
        div()
            .size_full()
            .relative()
            .flex()
            .when(self.image_pane_stacked, |el| el.flex_col())
            .overflow_hidden()
            .child(self.image_pane_layout_observer(cx))
            .child(chat)
            .children(self.render_image_pane(cx))
            .into_any_element()
    }
}

