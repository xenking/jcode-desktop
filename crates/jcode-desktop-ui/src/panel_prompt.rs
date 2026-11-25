use super::*;
use super::classify::{is_pinnable_prompt, is_user_turn};
use std::{cell::Cell, rc::Rc};

// The card sits below the row's inter-message gap. The sticky boundary is
// the card top, not the row top and not the disappearance of its bottom.
pub(super) const PROMPT_TOP_PADDING: f32 = 10.;
const PIN_DISCLOSURE_WIDTH: f32 = 22.;
const PIN_DISCLOSURE_GAP: f32 = 6.;

pub(super) fn fixture_items() -> Vec<Item> {
    vec![
        Item::User("A longer first line with a short second line\nShort second line.".into()),
        Item::User("Short first.\nA much longer second line that grows the rounded outline outward.".into()),
        Item::User("Please make this naturally wrapped prompt fit closely around every rendered line, keeping **bold text**, `inline code`, and [links](https://example.com) selectable without filling the unused space after the final words. Fin.".into()),
        Item::User("Unicode stays aligned: café 日本語 🦀\nSmall.\nA longer third line to exercise both inward and outward curves.".into()),
        Item::User("A separate paragraph stays separate.\n\nTiny.".into()),
        Item::User(super::voice::tag::wrap("Can you fix the flaky sidebar test and then rerun it")),
        Item::User("Please review this pasted specification and preserve the complete original prompt.\n".repeat(30)),
    ]
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VisibleRows {
    first: usize,
    last: usize,
    first_top: gpui::Pixels,
    next_prompt_top: Option<(usize, gpui::Pixels)>,
}

/// The list's logical offset is an end sentinel while following the tail, not
/// the first visible row. Observe real painted bounds instead of that offset.
pub(super) fn visibility_marker(
    row: usize,
    is_prompt: bool,
    first_visible: Rc<Cell<Option<VisibleRows>>>,
    list: ListState,
) -> gpui::AnyElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, _, _| {
            let viewport = list.viewport_bounds();
            if bounds.bottom() > viewport.top() && bounds.top() < viewport.bottom() {
                let top = bounds.top() - viewport.top();
                let mut visible = first_visible.get().unwrap_or(VisibleRows {
                    first: row,
                    last: row,
                    first_top: top,
                    next_prompt_top: None,
                });
                if row <= visible.first {
                    visible.first = row;
                    visible.first_top = top;
                }
                let card_top = top + px(PROMPT_TOP_PADDING);
                if is_prompt
                    && card_top > px(0.)
                    && visible.next_prompt_top.is_none_or(|(next, _)| row < next)
                {
                    visible.next_prompt_top = Some((row, card_top));
                }
                visible.last = visible.last.max(row);
                first_visible.set(Some(visible));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// Use painted row geometry, including partially clipped rows. Logical list
/// offsets are an end sentinel during tail following and cannot identify this.
fn prompt_for_viewport(
    prompt_rows: &[(usize, usize)],
    visible: Option<VisibleRows>,
    at_live_end: bool,
) -> Option<usize> {
    let visible = visible?;
    // A freshly submitted prompt at the live end supersedes the old reminder.
    // While browsing history, a later prompt entering the bottom must not unpin
    // the current turn before that later card reaches the top.
    if at_live_end
        && prompt_rows
            .last()
            .is_some_and(|(row, _)| *row > visible.first && *row <= visible.last)
    {
        return None;
    }
    prompt_rows
        .iter()
        .rev()
        .find(|(row, _)| {
            *row < visible.first
                || (*row == visible.first && visible.first_top + px(PROMPT_TOP_PADDING) <= px(0.))
        })
        .map(|(_, index)| *index)
}

/// Count user turns rather than transcript rows. Tool output, assistant text,
/// todo snapshots and restored background notices must not age prompt colors.
fn prompt_distance(items: &[Item], index: usize) -> Option<usize> {
    items.get(index).filter(|item| is_user_turn(item))?;
    Some(
        items
            .iter()
            .skip(index + 1)
            .filter(|item| is_user_turn(item))
            .count(),
    )
}

/// Oldest genuine prompt is one. Later arrivals never renumber history.
pub(super) fn prompt_number(items: &[Item], index: usize) -> Option<usize> {
    items.get(index).filter(|item| is_user_turn(item))?;
    Some(
        items[..=index]
            .iter()
            .filter(|item| is_user_turn(item))
            .count(),
    )
}


impl Panel {
    pub(super) fn clear_pinned_prompt(&mut self) {
        self.offscreen_prompt = None;
        self.offscreen_prompt_clip = None;
        self.expanded_pinned_prompt = None;
    }

    /// The transcript always renders the complete prompt. Only the pinned copy has disclosure.
    pub(super) fn render_user_prompt(
        &self,
        index: usize,
        text: &str,
        pinned_width: Option<gpui::Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let pinned = pinned_width.is_some();
        let now = Instant::now();
        // Animate position only. Fading the whole card exposes transcript text
        // underneath a sticky prompt during pending/accept acknowledgement.
        let (offset, animating) = self
            .accepted_users
            .get(&index)
            .map(|at| {
                let (offset, _, animating) = crate::ack::motion(*at, now);
                (offset, animating)
            })
            .unwrap_or((0.0, false));
        if animating {
            window.request_animation_frame();
        }
        let clean_chat = self.clean_chat_enabled();
        let number = self.absolute_prompt_number(index);
        // Keep the number outside the card in a compact circular badge. Wider
        // turn numbers grow into a pill without taking space from card padding.
        let number_width = number.map_or(0., |number| {
            (number.to_string().len() as f32 * 7. + 8.).max(20.)
        });
        let background = Theme::global()
            .prompt_background(prompt_distance(&self.items, index).unwrap_or(usize::MAX));
        // The model receives `<transcription>` tags. People see a pill instead.
        let (visible, transcribed) = super::voice::tag::strip(text);
        let text = visible.as_ref();
        let clock = (!clean_chat).then(|| self.user_clock(index)).flatten();
        let mut unresolved = Vec::new();
        let attachments: HashMap<_, _> = crate::image_chip::image_marker_ranges(text)
            .into_iter()
            .enumerate()
            .map(|(slot, range)| {
                let marker = &text[range];
                let image =
                    self.prompt_image_index(index, slot, marker)
                        .and_then(|index| match self.items.get(index) {
                            Some(Item::Image(image)) => Some(image),
                            _ => None,
                        });
                let key = crate::image_chip::session_image_key(marker);
                let number = self.gallery_image_ordinal(marker)
                    .or_else(|| image.and_then(TranscriptImage::image_ordinal));
                if let Some(key) =
                    key.filter(|key| number.is_none() && self.may_request_prompt_image_ordinal(key))
                {
                    unresolved.push(key.to_owned());
                }
                if let Some(image) = image {
                    self.request_history_thumbnail(image, false);
                }
                (
                    marker.to_owned(),
                    markdown::PromptImage {
                        slot,
                        label: crate::image_chip::prompt_label(marker, number),
                        preview: image
                            .and_then(|image| self.gallery_image_asset(image))
                            .or_else(|| key.and_then(|key| self.history_image_asset(key))),
                    },
                )
            })
            .collect();
        let panel = cx.entity().downgrade();
        if !unresolved.is_empty() {
            let panel = panel.clone();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, _| {
                    panel.request_prompt_image_ordinals(unresolved)
                });
            });
        }
        let attachments = Rc::new(attachments);
        let prompt_images = markdown::PromptImages {
            attachments: attachments.clone(),
            on_click: Rc::new(move |marker, window, cx| {
                let Some(attachment) = attachments.get(marker) else {
                    return;
                };
                let _ = panel.update(cx, |panel, cx| {
                    if let Some(image_index) = panel.prompt_image_index(index, attachment.slot, marker)
                        && let Some(Item::Image(image)) = panel.items.get(image_index)
                    {
                        panel.open_image_preview(image.clone(), window, cx);
                    } else if let Some(key) = crate::image_chip::session_image_key(marker) {
                        panel.open_history_image_key(key, window, cx);
                    }
                });
            }),
        };
        let (body, toggle) = markdown::with_prompt_images(prompt_images, || {
            let preview = pinned_width.map(|width| {
                let time_width = clock.as_ref().map_or(px(0.), |clock| {
                    window
                        .text_system()
                        .shape_line(
                            clock.clone().into(),
                            px(10.),
                            &[gpui::TextRun {
                                len: clock.len(),
                                font: gpui::font(Theme::global().FONT_MONO),
                                ..Default::default()
                            }],
                            None,
                        )
                        .width
                        .ceil()
                        + px(6.)
                });
                let badge_width = if number.is_some() {
                    number_width + 6.
                } else {
                    0.
                };
                let width = (width - px(16. + badge_width) - time_width).max(px(1.));
                let preview = markdown::PinnedPromptPreview::new(text);
                let long = preview.overflows(preview.visible_prefix(width, window));
                let width = if long {
                    (width - px(PIN_DISCLOSURE_WIDTH + PIN_DISCLOSURE_GAP)).max(px(1.))
                } else {
                    width
                };
                (preview, long, width)
            });
            let expanded = self.expanded_pinned_prompt == Some(index);
            let long = preview.as_ref().is_some_and(|(_, long, _)| *long);
            let body = if let Some((preview, _, width)) = preview.filter(|_| !expanded || !long) {
                div()
                    .max_w(width)
                    .child(markdown::PinnedPromptPreview::render(
                        preview.visible_prefix(width, window),
                        &self.transcript_selection,
                        format!("pinned-preview-{index}").into(),
                        window,
                        cx,
                        background,
                    ))
                    .into_any_element()
            } else {
                markdown::render_prompt_with_prefix(
                    text,
                    index,
                    &if pinned {
                        format!("pinned-{index}")
                    } else {
                        index.to_string()
                    },
                    &self.transcript_selection,
                    window,
                    cx,
                    self.media_preview_handler(cx),
                    background,
                )
            };
            (body, long.then_some(expanded))
        });
        let body = div()
            .relative()
            .min_w_0()
            .debug_selector(move || format!("prompt-content-{index}"))
            .child(body);
        let body = if toggle == Some(true) {
            let height = self.transcript_list.viewport_bounds().size.height;
            crate::transcript_scroll::vertical(
                div()
                    .id(SharedString::from(format!("pinned-body-{index}")))
                    .max_h(height * 0.65)
                    .child(body),
            )
            .into_any_element()
        } else {
            body.into_any_element()
        };
        let body = if let Some(expanded) = toggle {
            div()
                .flex()
                .items_start()
                .gap(px(PIN_DISCLOSURE_GAP))
                .child(div().min_w_0().flex_shrink_1().child(body))
                .child(self.pinned_prompt_disclosure(index, expanded, cx))
                .into_any_element()
        } else {
            body
        };
        let card = div()
            .relative()
            .debug_selector(move || format!("user-prompt-{index}"))
            .max_w_full()
            .min_w_0()
            .flex_shrink_1()
            .flex()
            .flex_col()
            .px(px(8.))
            .py_1()
            .text_color(Theme::global().TEXT_USER)
            .child(body)
            .when(transcribed && !clean_chat && !pinned, |card| {
                let theme = Theme::global();
                card.child(
                    div()
                        .debug_selector(move || format!("prompt-transcribed-{index}"))
                        .mt_1()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .self_start()
                        .px(px(7.))
                        .h(px(18.))
                        .rounded_full()
                        .bg(theme.INLINE_CODE_BG)
                        .text_size(px(10.))
                        .text_color(theme.TEXT_DIM)
                        .child(
                            gpui::svg()
                                .data(include_bytes!("../../../assets/icons/microphone.svg")
                                    as &'static [u8])
                                .text_color(theme.TEXT_DIM)
                                .size(px(10.)),
                        )
                        .child("Transcribed"),
                )
            })
            .into_any_element();
        // Animate and hide the complete row so inline and sticky badges always
        // travel with their cards. Shrink only the card when markdown wraps.
        div()
            .flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .items_start()
            .relative()
            .left(px(offset))
            .when_some(number, |row, number| {
                row.gap(px(6.)).child(
                    div()
                        .debug_selector(move || format!("prompt-number-{index}-{number}"))
                        .flex_none()
                        .mt(px(4.))
                        .w(px(number_width))
                        .h(px(20.))
                        .rounded_full()
                        .bg(background)
                        .font_family(Theme::global().FONT_MONO)
                        .text_center()
                        .text_size(px(10.))
                        .line_height(px(20.))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(number.to_string()),
                )
            })
            .child(card)
            .when_some(clock, |row, clock| {
                // A quiet arrival time beside the card, from the message record.
                row.child(
                    div()
                        .debug_selector(move || format!("prompt-time-{index}"))
                        .flex_none()
                        .ml(px(6.))
                        .mt(px(7.))
                        .font_family(Theme::global().FONT_MONO)
                        .text_size(px(10.))
                        .text_color(Theme::global().TEXT_FAINT)
                        .child(clock),
                )
            })
            .into_any_element()
    }

    fn pinned_prompt_disclosure(
        &self,
        index: usize,
        expanded: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let id: SharedString = format!("prompt-expand-{index}").into();
        let label = if expanded {
            "Collapse pinned prompt"
        } else {
            "Expand pinned prompt"
        };
        let theme = Theme::global();
        crate::keyboard::button(
            div()
                .id(id.clone())
                .debug_selector(move || id.to_string())
                .relative()
                .flex_none()
                .w(px(PIN_DISCLOSURE_WIDTH))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .bg(theme.INLINE_CODE_BG)
                .text_color(theme.TEXT_DIM)
                .hover(|el| el.text_color(theme.TEXT))
                .role(gpui::Role::Button)
                .aria_label(label)
                .aria_expanded(expanded)
                .child(if expanded { "▴" } else { "▾" })
                .children(
                    (index == 1 && crate::fixture_control_targets::transcript_scroll_fixture())
                        .then(|| {
                            crate::fixture_control_targets::marker(
                                || "prompt:toggle".into(),
                                || Some(self.session_id.clone()),
                            )
                            .unwrap()
                        }),
                )
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    if panel.offscreen_prompt == Some(index) {
                        panel.expanded_pinned_prompt = (!expanded).then_some(index);
                        cx.notify();
                    }
                })),
        )
    }

    /// The out-of-flow anchor provides the actual pane width, even during a
    /// resize. The overlay never becomes a transcript row or consumes space.
    pub(super) fn render_pinned_prompt(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.rewind_selection.is_some() {
            return None;
        }
        let index = self.offscreen_prompt?;
        let panel = cx.entity().downgrade();
        Some(
            gpui::canvas(
                move |bounds, window, cx| {
                    let mut overlay = panel
                        .update(cx, |panel, cx| {
                            panel.render_pinned_prompt_at_width(
                                index,
                                bounds.size.width,
                                window,
                                cx,
                            )
                        })
                        .ok()??;
                    overlay.prepaint_as_root(
                        bounds.origin,
                        gpui::size(
                            gpui::AvailableSpace::Definite(bounds.size.width),
                            gpui::AvailableSpace::MaxContent,
                        ),
                        window,
                        cx,
                    );
                    Some(overlay)
                },
                |_, overlay, window, cx| {
                    if let Some(mut overlay) = overlay {
                        overlay.paint(window, cx);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(0.))
            .into_any_element(),
        )
    }

    fn render_pinned_prompt_at_width(
        &self,
        index: usize,
        width: gpui::Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let item = self.items.get(index)?;
        if !is_pinnable_prompt(item) {
            return None;
        }
        let Item::User(text) = item else {
            return None;
        };
        let background = Theme::global().panel_background(self.surface_focused);
        Some(
            div()
                .id(SharedString::from(format!("pinned-latest-prompt-{index}")))
                .debug_selector(|| "pinned-latest-prompt".into())
                .relative()
                .w_full()
                .min_w_0()
                // Mask transcript ink behind the prompt. The separate fade
                // below starts fully opaque, avoiding the hard edge where a
                // solid backdrop used to meet a partially transparent shadow.
                .bg(background)
                .max_h(
                    self.offscreen_prompt_clip
                        .map(|top| (top - px(6.)).max(px(0.)))
                        .unwrap_or(self.transcript_list.viewport_bounds().size.height)
                        .min(self.transcript_list.viewport_bounds().size.height),
                )
                .when(self.offscreen_prompt_clip.is_some(), |overlay| {
                    // Bottom alignment moves the intact outgoing card upward
                    // as the next one approaches, rather than cutting through
                    // stationary text. Keep a visible gap between both cards.
                    overlay.flex().flex_col().justify_end().overflow_hidden()
                })
                .block_mouse_except_scroll()
                .px(px(12.))
                .text_size(px(markdown::PINNED_PROMPT_FONT_SIZE))
                .child(self.render_user_prompt(index, text, Some(width - px(24.)), window, cx))
                .map(|prompt| {
                    let transparent = gpui::Rgba {
                        a: 0.,
                        ..background
                    };
                    let fade_height = if self.offscreen_prompt_clip.is_some() {
                        6.
                    } else {
                        18.
                    };
                    div()
                        .relative()
                        .w_full()
                        .child(prompt)
                        // Outside the prompt's scroll clip so the soft edge
                        // remains visible without changing transcript layout.
                        .child(
                            div()
                                .debug_selector(|| "pinned-prompt-fade".into())
                                .absolute()
                                .left_0()
                                .bottom(px(-fade_height))
                                .w_full()
                                .h(px(fade_height))
                                .bg(gpui::linear_gradient(
                                    180.,
                                    gpui::linear_color_stop(background, 0.),
                                    gpui::linear_color_stop(transparent, 1.),
                                )),
                        )
                })
                .into_any_element(),
        )
    }

    /// Read visibility after the list paints. Measuring during render would
    /// still describe the previous scroll position, width, or transcript.
    pub(super) fn prompt_visibility_observer(
        &self,
        prompt_rows: Arc<Vec<(usize, usize)>>,
        first_visible: Rc<Cell<Option<VisibleRows>>>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let panel = cx.entity().downgrade();
        let following_tail = self.stick_to_bottom;
        let row_count = self.transcript_list.item_count();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, _, cx| {
                let visible = first_visible.take();
                let at_live_end =
                    following_tail || visible.is_some_and(|rows| rows.last + 1 == row_count);
                let offscreen = prompt_for_viewport(&prompt_rows, visible, at_live_end);

                // The next card pushes the old reminder above the viewport.
                // Constrain only the overlay, never either card's layout.
                let clip = offscreen.and_then(|_| visible?.next_prompt_top.map(|(_, top)| top));
                cx.defer(move |cx| {
                    // Read first: an update counts as a change for retained views.
                    let Some(entity) = panel.upgrade() else {
                        return;
                    };
                    let current = entity.read(cx);

                    if current.offscreen_prompt == offscreen
                        && current.offscreen_prompt_clip == clip
                    {
                        return;
                    }
                    entity.update(cx, |panel, cx| {
                        if panel.offscreen_prompt != offscreen {
                            panel.expanded_pinned_prompt = None;
                        }
                        panel.offscreen_prompt = offscreen;
                        panel.offscreen_prompt_clip = clip;
                        cx.notify();
                    });
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel_transcript(vcx: &mut gpui::VisualTestContext, delta: f32) {
        let viewport = vcx.debug_bounds("transcript").unwrap();
        let position = gpui::point(viewport.left() + px(2.), viewport.center().y);
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
    }

    #[gpui::test]
    fn pinned_only_dictation_disclosure_keeps_the_real_transcript_anchor(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::input::bind_keys);
        for (width, clean_chat) in [(320., false), (900., true)] {
            let (panel, vcx) = cx.add_window_view(|_, cx| {
                Panel::new(
                    "pinned-only".into(),
                    None,
                    None,
                    crate::harness::spawn_inert(),
                    cx,
                )
            });
            let handle = vcx.update(|window, _| window.window_handle());
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.)));
            let text = format!(
                "[#image 7] {}Unabridged dictation tail.",
                "Keep the dictation fully visible when it wraps at the edge. ".repeat(24)
            );
            panel.update(vcx, |panel, cx| {
                panel.set_clean_chat(Some(clean_chat), cx);
                panel
                    .items
                    .push(Item::User(super::super::voice::tag::wrap(text.trim())));
                panel
                    .items
                    .extend((0..75).map(|n| Item::Assistant(format!("Answer {n}"))));
                panel.stick_to_bottom = false;
                cx.notify();
            });
            vcx.run_until_parked();
            let inline = vcx.debug_bounds("prompt-content-0").unwrap();
            assert!(
                inline.size.height > px(160.),
                "all inline dictation lines are laid out at {width}px"
            );
            assert!(vcx.debug_bounds("prompt-expand-0").is_none());
            assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());

            for _ in 0..10 {
                wheel_transcript(vcx, -180.);
                if vcx.debug_bounds("transcript-row-0").is_none() {
                    break;
                }
            }
            let pinned = vcx
                .debug_bounds("pinned-latest-prompt")
                .expect("wheel pins the offscreen prompt");
            let compact = vcx.debug_bounds("prompt-content-0").unwrap();
            assert!(compact.size.height <= px(64.));
            assert!(
                crate::image_chip::take_transcript_chips()
                    .iter()
                    .any(|(label, bounds)| label == "[Image #7]" && compact.contains(&bounds.center()))
            );
            let toggle = vcx.debug_bounds("prompt-expand-0").unwrap();
            assert!(toggle.size.width < pinned.size.width / 2.);
            assert!(
                toggle.top() < compact.bottom(),
                "disclosure sits beside the preview"
            );

            let anchor = |vcx: &gpui::VisualTestContext| {
                panel.read_with(vcx, |panel, _| {
                    let top = panel.transcript_list.logical_scroll_top();
                    (
                        top.item_ix,
                        top.offset_in_item,
                        panel.transcript_list.viewport_bounds(),
                    )
                })
            };
            let before = anchor(vcx);
            vcx.simulate_click(toggle.center(), Default::default());
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("prompt-content-0").unwrap().size.height > compact.size.height
            );
            assert_eq!(
                anchor(vcx),
                before,
                "opening the sticky copy must not jump to the row"
            );
            vcx.simulate_keystrokes("space");
            vcx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse("space").unwrap(),
            });
            vcx.run_until_parked();
            assert_eq!(
                vcx.debug_bounds("prompt-content-0").unwrap().size.height,
                compact.size.height
            );
            assert_eq!(
                anchor(vcx),
                before,
                "closing the sticky copy must not move the list"
            );
            vcx.simulate_keystrokes("enter");
            vcx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse("enter").unwrap(),
            });
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("prompt-content-0").unwrap().size.height > compact.size.height
            );

            for _ in 0..10 {
                wheel_transcript(vcx, 300.);
                if vcx.debug_bounds("pinned-latest-prompt").is_none() {
                    break;
                }
            }
            assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());
            assert_eq!(
                vcx.debug_bounds("prompt-content-0").unwrap().size.height,
                inline.size.height
            );
            assert!(vcx.debug_bounds("prompt-expand-0").is_none());
            let leaf = vcx.debug_bounds("selectable-text-0-0").unwrap();
            let position = gpui::point(leaf.left() + px(100.), leaf.top() + px(10.));
            vcx.simulate_event(gpui::MouseDownEvent {
                button: gpui::MouseButton::Left,
                position,
                click_count: 4,
                ..Default::default()
            });
            vcx.simulate_event(gpui::MouseUpEvent {
                button: gpui::MouseButton::Left,
                position,
                click_count: 4,
                ..Default::default()
            });
            vcx.simulate_keystrokes("ctrl-c");
            assert!(
                vcx.update(|_, cx| cx.read_from_clipboard())
                    .and_then(|item| item.text())
                    .is_some_and(|text| text.contains("Unabridged dictation tail."))
            );
            for _ in 0..10 {
                wheel_transcript(vcx, -180.);
                if vcx.debug_bounds("transcript-row-0").is_none() {
                    break;
                }
            }
            assert_eq!(
                vcx.debug_bounds("prompt-content-0").unwrap().size.height,
                compact.size.height
            );
            println!(
                "ACCEPTANCE: {width}px clean={clean_chat} inline={inline:?} preview={compact:?}, wheel pin/unpin/repin, click/Space/Enter, transcript anchor unchanged"
            );
        }
    }

    #[gpui::test]
    fn pinned_only_disclosure_starts_at_four_real_lines(cx: &mut gpui::TestAppContext) {
        for (width, text, overflows) in [
            (600., "First line\nSecond line\nThird line", false),
            (
                600.,
                "First line\nSecond line\nThird line\nFourth line",
                true,
            ),
            (
                900.,
                "First line\nSecond line\nThe last line is just long enough to wrap in a narrow pane.",
                false,
            ),
            (
                320.,
                "First line\nSecond line\nThe last line is just long enough to wrap in a narrow pane.",
                true,
            ),
        ] {
            let (panel, vcx) = cx.add_window_view(|_, cx| {
                Panel::new(
                    "pinned-lines".into(),
                    None,
                    None,
                    crate::harness::spawn_inert(),
                    cx,
                )
            });
            let handle = vcx.update(|window, _| window.window_handle());
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(550.)));
            panel.update(vcx, |panel, cx| {
                panel.items.push(Item::User(text.into()));
                panel
                    .items
                    .extend((0..30).map(|n| Item::Assistant(format!("Answer {n}"))));
                cx.notify();
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("pinned-latest-prompt").is_some());
            assert_eq!(
                vcx.debug_bounds("prompt-expand-0").is_some(),
                overflows,
                "{width}px: {text}"
            );
        }
    }

    const BACKGROUND_NOTICES: [&str; 4] = [
        "**Background task started** `task-42` · `Workspace tests`",
        "**Background task progress** `task-42` · `Workspace tests` (`bash`)\n\n35% · Running tests",
        "**Background task** `task-42` · `Workspace tests` (`bash`) · ✓ completed · 8.2s · exit 0",
        "**Background task stalled** `task-42` · `Workspace tests` (`bash`) · no output or progress for 30s (running 60s total)",
    ];

    #[test]
    fn prompt_numbers_ignore_notices_and_remain_stable_on_append() {
        let mut items = vec![Item::User("First".into()), Item::Assistant("Answer".into())];
        for notice in BACKGROUND_NOTICES {
            items.push(Item::User(notice.into()));
            assert_eq!(prompt_number(&items, items.len() - 1), None);
        }
        items.push(Item::User("  ".into()));
        assert_eq!(prompt_number(&items, items.len() - 1), None);
        assert_eq!(prompt_number(&items, 1), None);
        assert_eq!(prompt_number(&items, items.len()), None);
        for number in 2..=15 {
            items.push(Item::User(format!("Prompt {number}")));
            assert_eq!(prompt_number(&items, items.len() - 1), Some(number));
            assert_eq!(prompt_number(&items, 0), Some(1));
        }
    }

    fn assert_number_geometry(vcx: &mut gpui::VisualTestContext, card: gpui::Bounds<gpui::Pixels>) {
        let number = vcx
            .debug_bounds("prompt-number-0-1")
            .expect("genuine prompt is numbered");
        assert_eq!(
            card.left() - number.right(),
            px(6.),
            "badge sits left of card"
        );
        assert_eq!(number.top(), card.top() + px(4.));
        assert_eq!(number.size, gpui::size(px(20.), px(20.)));
        assert!(number.left() >= vcx.debug_bounds("transcript").unwrap().left());
        let content = vcx.debug_bounds("prompt-content-0").unwrap();
        assert!(
            number.right() < content.left(),
            "number never overlaps prompt content"
        );
        assert_eq!(
            card.size.height,
            content.size.height + px(8.),
            "number adds no footer height"
        );
        assert_eq!(
            card.size.width,
            content.size.width
                + px(16.)
                + vcx
                    .debug_bounds("prompt-expand-0")
                    .map_or(px(0.), |toggle| {
                        toggle.size.width + px(PIN_DISCLOSURE_GAP)
                    }),
            "number does not change card padding"
        );
    }

    #[test]
    fn background_notifications_are_never_pinnable_prompts() {
        for notice in BACKGROUND_NOTICES {
            assert!(!is_pinnable_prompt(&Item::User(format!("\n{notice}\n"))));
        }
        assert!(!is_pinnable_prompt(&Item::User("  ".into())));
        assert!(is_pinnable_prompt(&Item::User(
            "Explain background tasks".into()
        )));
        assert!(!is_pinnable_prompt(&Item::BackgroundTask {
            task_id: "task-42".into(),
            label: "Workspace tests".into(),
            summary: "Running tests".into(),
            detail: None,
            percent: Some(35.),
            done: false,
        }));
    }

    #[test]
    fn only_real_user_turns_age_prompt_card_backgrounds() {
        let mut items = vec![Item::User("First prompt".into())];
        assert_eq!(prompt_distance(&items, 0), Some(0));
        items.push(Item::Assistant("Response".into()));
        items.push(Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: "{}".into(),
            output: "Done".into(),
            done: true,
            error: None,
        });
        for notice in BACKGROUND_NOTICES {
            items.push(Item::User(notice.into()));
            assert_eq!(prompt_distance(&items, items.len() - 1), None);
        }
        items.push(Item::BackgroundTask {
            task_id: "task".into(),
            label: "Tests".into(),
            summary: "Running".into(),
            detail: None,
            percent: None,
            done: false,
        });
        assert_eq!(prompt_distance(&items, 0), Some(0));
        items.push(Item::User("Second prompt".into()));
        assert_eq!(prompt_distance(&items, 0), Some(1));
        assert_eq!(prompt_distance(&items, items.len() - 1), Some(0));
        items.push(Item::User("Third prompt".into()));
        assert_eq!(prompt_distance(&items, 0), Some(2));
        assert_eq!(prompt_distance(&items, items.len() - 1), Some(0));
        assert_eq!(prompt_distance(&items, items.len()), None);
    }

    #[gpui::test]
    fn background_notifications_stay_in_the_scroller(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "background-no-pin".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(400.)));
        for notice in BACKGROUND_NOTICES {
            for real_prompt in [false, true] {
                panel.update(vcx, |panel, cx| {
                    panel.items.clear();
                    if real_prompt {
                        panel.items.push(Item::User("Run the tests".into()));
                    }
                    panel.items.push(Item::User(notice.into()));
                    for i in 0..40 {
                        panel
                            .items
                            .push(Item::Assistant(format!("Response paragraph {i}")));
                    }
                    cx.notify();
                });
                vcx.run_until_parked();
                panel.read_with(vcx, |panel, _| {
                    assert_eq!(panel.offscreen_prompt, real_prompt.then_some(0));
                    assert!(
                        panel.transcript_render_rows().iter().any(|row| {
                            matches!(row.source, TranscriptRowSource::Settled(index)
                            if matches!(&panel.items[index], Item::User(text) if text == notice))
                        }),
                        "background output remains in transcript history"
                    );
                });
                assert_eq!(
                    vcx.debug_bounds("pinned-latest-prompt").is_some(),
                    real_prompt
                );
            }
        }
    }

    fn visible(first: usize, last: usize, first_top: f32) -> Option<VisibleRows> {
        Some(VisibleRows {
            first,
            last,
            first_top: px(first_top),
            next_prompt_top: None,
        })
    }

    #[test]
    fn viewport_prompt_never_comes_from_a_future_turn() {
        let prompts = [(0, 0), (12, 16), (30, 40)];
        assert_eq!(prompt_for_viewport(&prompts, None, false), None);
        for (first, expected) in [
            (0, None),
            (5, Some(0)),
            (12, Some(0)),
            (20, Some(16)),
            (35, Some(40)),
        ] {
            assert_eq!(
                prompt_for_viewport(&prompts, visible(first, first + 5, 0.), false),
                expected
            );
        }
    }

    #[test]
    fn pin_threshold_is_the_card_top_even_while_its_row_is_visible() {
        let prompts = [(0, 0), (12, 16)];
        for (top, expected) in [
            (0., None),
            (-9.99, None),
            (-10., Some(0)),
            (-10.01, Some(0)),
            (-35., Some(0)),
        ] {
            assert_eq!(
                prompt_for_viewport(&prompts, visible(0, 5, top), false),
                expected
            );
        }
        // The previous card survives the incoming row's ten-pixel top gap,
        // in both directions, until the incoming card itself reaches zero.
        for (top, expected) in [
            (0., 0),
            (-9.5, 0),
            (-10., 16),
            (-10.5, 16),
            (-9.5, 0),
            (0., 0),
        ] {
            assert_eq!(
                prompt_for_viewport(&prompts, visible(12, 15, top), false),
                Some(expected)
            );
        }
        // A subsequent prompt down-screen does not remove historical context.
        assert_eq!(
            prompt_for_viewport(&prompts, visible(3, 12, -5.), false),
            Some(0)
        );
        // At the live end, a newly visible submitted prompt supersedes it.
        assert_eq!(
            prompt_for_viewport(&prompts, visible(3, 12, -5.), true),
            None
        );
        assert_eq!(
            prompt_for_viewport(&prompts, visible(12, 15, -10.), false),
            Some(16)
        );
    }

    #[gpui::test]
    fn sticky_card_keeps_original_geometry_at_partial_scroll_threshold(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "sticky-geometry".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [600., 320.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(800.)));
            for text in [
                "Hi".to_string(),
                "A **formatted** prompt that wraps naturally. ".repeat(12),
            ] {
                panel.update(vcx, |panel, cx| {
                    *panel.items = vec![Item::User(text.clone())];
                    for n in 0..40 {
                        panel
                            .items
                            .push(Item::Assistant(format!("Response paragraph {n}")));
                    }
                    panel.stick_to_bottom = false;
                    panel.transcript_list.scroll_to(gpui::ListOffset::default());
                    cx.notify();
                });
                vcx.run_until_parked();
                panel.update(vcx, |panel, cx| {
                    panel.stick_to_bottom = false;
                    panel.transcript_list.scroll_to(gpui::ListOffset::default());
                    cx.notify();
                });
                vcx.run_until_parked();
                let original = vcx.debug_bounds("user-prompt-0").unwrap();
                assert_number_geometry(vcx, original);
                let viewport = vcx.debug_bounds("transcript").unwrap();
                assert_eq!(original.top(), viewport.top() + px(PROMPT_TOP_PADDING));
                // Cross in both directions, including an exactly aligned card,
                // fractional pixel offsets, and a still mostly visible row.
                for offset in [9.5, 10., 10.5, 15., 10., 9.5, 0.] {
                    panel.update(vcx, |panel, cx| {
                        panel.transcript_list.scroll_to(gpui::ListOffset {
                            item_ix: 0,
                            offset_in_item: px(offset),
                        });
                        cx.notify();
                    });
                    vcx.run_until_parked();
                    let card = vcx
                        .debug_bounds("user-prompt-0")
                        .expect("card never disappears");
                    assert_number_geometry(vcx, card);
                    if vcx.debug_bounds("prompt-expand-0").is_some() {
                        assert!(card.size.height < original.size.height);
                    } else {
                        assert_eq!(
                            card.size, original.size,
                            "short card stays the same size at {width}px, offset {offset}"
                        );
                    }
                    assert_eq!(card.left(), original.left(), "no horizontal jump");
                    assert_eq!(
                        card.top(),
                        (original.top() - px(offset)).max(viewport.top()),
                        "card clamps exactly to transcript top"
                    );
                    assert_eq!(vcx.debug_bounds("transcript").unwrap(), viewport);
                    assert_eq!(
                        vcx.debug_bounds("pinned-latest-prompt").is_some(),
                        offset >= PROMPT_TOP_PADDING
                    );
                    if let Some(pinned) = vcx.debug_bounds("pinned-latest-prompt") {
                        assert_eq!(pinned.top(), card.top(), "no banner padding above the card");
                        assert_eq!(
                            pinned.size.height, card.size.height,
                            "sticky wrapper must neither clip the card to a banner nor add bottom padding"
                        );
                    }
                    assert!(
                        vcx.debug_bounds("transcript-row-0").is_some(),
                        "threshold must be reached before the prompt row disappears"
                    );
                }
            }
        }
    }

    #[gpui::test]
    fn prompt_cards_fit_content_without_captions(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "compact-prompts".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(800.)));
        for width in [600., 320.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(800.)));
            for text in [
                "Hi".to_string(),
                "A longer prompt with **formatted text** that should wrap naturally. ".repeat(5),
            ] {
                panel.update(vcx, |panel, cx| {
                    *panel.items = vec![Item::User(text.clone())];
                    cx.notify();
                });
                vcx.run_until_parked();
                let card = vcx.debug_bounds("user-prompt-0").expect("prompt paints");
                assert_number_geometry(vcx, card);
                let viewport = vcx.debug_bounds("transcript").unwrap();
                assert!(card.left() >= viewport.left());
                assert!(
                    card.right() <= viewport.right(),
                    "prompt stays within chat width"
                );
                assert!(vcx.debug_bounds("role-caption-you, 1").is_none());
                if text == "Hi" {
                    assert!(
                        card.size.width < px(100.),
                        "short prompt hugs its content: {card:?}"
                    );
                } else {
                    assert!(card.size.height > px(50.), "long prompt wraps");
                }
            }
        }
    }

    #[gpui::test]
    fn multi_digit_prompt_numbers_stay_in_left_badges(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "prompt-number-gutter".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [600., 320.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(800.)));
            for text in [
                "Hi".to_string(),
                "A **formatted** prompt with wrapping content. ".repeat(8),
            ] {
                for (count, card_selector, content_selector, number_selector) in [
                    (
                        99,
                        "user-prompt-98",
                        "prompt-content-98",
                        "prompt-number-98-99",
                    ),
                    (
                        100,
                        "user-prompt-99",
                        "prompt-content-99",
                        "prompt-number-99-100",
                    ),
                ] {
                    panel.update(vcx, |panel, cx| {
                        *panel.items = (1..count).map(|_| Item::User("Earlier".into())).collect();
                        panel.items.push(Item::User(text.clone()));
                        panel.stick_to_bottom = true;
                        panel.transcript_list.scroll_to_end();
                        cx.notify();
                    });
                    vcx.run_until_parked();
                    let card = vcx.debug_bounds(card_selector).unwrap();
                    let content = vcx.debug_bounds(content_selector).unwrap();
                    let number = vcx.debug_bounds(number_selector).unwrap();
                    assert!(
                        number.right() < content.left(),
                        "multi-digit number never overlaps markdown"
                    );
                    assert_eq!(number.right() + px(6.), card.left());
                    assert_eq!(number.top(), card.top() + px(4.));
                    assert_eq!(number.size.width, px(if count == 99 { 22. } else { 29. }));
                    assert!(number.left() >= vcx.debug_bounds("transcript").unwrap().left());
                    assert_eq!(card.size.width, content.size.width + px(16.));
                    assert_eq!(
                        card.size.height,
                        content.size.height + px(8.),
                        "no numbered footer"
                    );
                    assert!(card.right() <= vcx.debug_bounds("transcript").unwrap().right());
                }
            }
        }
    }

    #[gpui::test]
    fn newer_prompt_pushes_old_sticky_card_without_overlap(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "prompt-stack".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(600.)));
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![
                Item::User("Old prompt line\n\n".repeat(5)),
                Item::Assistant("Short answer".into()),
                Item::User("Newer prompt line".into()),
            ];
            for n in 0..40 {
                panel.items.push(Item::Assistant(format!("Response {n}")));
            }
            cx.notify();
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = false;
            panel.transcript_list.scroll_to(gpui::ListOffset::default());
            cx.notify();
        });
        vcx.run_until_parked();
        let mut old_pinned_size = None;
        let new_size = vcx.debug_bounds("user-prompt-2").unwrap().size;
        let mut pushed_off = false;
        let mut switched = false;
        for _ in 0..150 {
            let viewport = vcx.debug_bounds("transcript").unwrap();
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6.))),
                modifiers: Default::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
            scroll_momentum_tests::settle(vcx);
            let pinned_index = panel.read_with(vcx, |panel, _| panel.offscreen_prompt);
            if pushed_off {
                assert!(
                    pinned_index.is_some(),
                    "no missing reminder at the incoming row padding"
                );
            }
            if pinned_index == Some(0) {
                let old = vcx.debug_bounds("user-prompt-0").unwrap();
                let newer = vcx.debug_bounds("user-prompt-2").unwrap();
                assert_eq!(
                    old.size,
                    *old_pinned_size.get_or_insert(old.size),
                    "clipping never resizes the pinned content"
                );
                assert_eq!(newer.size, new_size);
                assert!(old.bottom() <= newer.top(), "prompt cards never overlap");
                if old.top() < viewport.top() {
                    pushed_off = true;
                    let overlay = vcx.debug_bounds("pinned-latest-prompt").unwrap();
                    assert_eq!(
                        overlay.bottom(),
                        (newer.top() - px(6.)).max(viewport.top()),
                        "outgoing card keeps a gap before the incoming card"
                    );
                    assert!(
                        newer.top() > viewport.top(),
                        "old stays pinned until newer reaches top"
                    );
                }
            } else if pinned_index == Some(2) {
                assert!(
                    pushed_off,
                    "incoming prompt pushes the old card upward before switching"
                );
                let newer = vcx.debug_bounds("user-prompt-2").unwrap();
                assert_eq!(newer.top(), viewport.top());
                assert_eq!(newer.size, new_size);
                switched = true;
                break;
            }
        }
        assert!(
            pushed_off && switched,
            "native scroll exercises push-off and handoff"
        );
        let mut restored = false;
        for _ in 0..100 {
            let viewport = vcx.debug_bounds("transcript").unwrap();
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(6.))),
                modifiers: Default::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
            scroll_momentum_tests::settle(vcx);
            if panel.read_with(vcx, |panel, _| panel.offscreen_prompt) == Some(0) {
                let old = vcx.debug_bounds("user-prompt-0").unwrap();
                let newer = vcx.debug_bounds("user-prompt-2").unwrap();
                assert_eq!(Some(old.size), old_pinned_size);
                assert_eq!(newer.size, new_size);
                assert!(newer.top() > viewport.top());
                assert!(old.top() < viewport.top());
                assert!(old.bottom() <= newer.top(), "reverse scroll never overlaps");
                assert_eq!(
                    vcx.debug_bounds("pinned-latest-prompt").unwrap().bottom(),
                    (newer.top() - px(6.)).max(viewport.top())
                );
                restored = true;
                break;
            }
        }
        assert!(
            restored,
            "reverse native scroll restores the pushed-off card without overlap"
        );
    }

    #[gpui::test]
    fn native_scroll_paints_historical_prompt_cards(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "native-prompt-acceptance".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(500.)));
        panel.update(vcx, |panel, cx| {
            for number in 1..=13 {
                panel
                    .items
                    .push(Item::User(format!("Distinct prompt {number}")));
                for line in 0..40 {
                    panel.items.push(Item::Assistant(format!(
                        "Answer {number}, paragraph {line}"
                    )));
                }
            }
            cx.notify();
        });
        vcx.run_until_parked();
        let pinned = vcx
            .debug_bounds("pinned-latest-prompt")
            .expect("tail prompt paints");
        let caption = vcx
            .debug_bounds("user-prompt-492")
            .expect("prompt card paints");
        assert!(caption.top() >= pinned.top() && caption.bottom() <= pinned.bottom());
        println!("ACCEPTANCE: live-end pinned card paints prompt 13");

        // Enter through the actual scroll event handler, not ListState mutation.
        let mut found_historical = false;
        for _ in 0..80 {
            let position = vcx.debug_bounds("transcript").unwrap().center();
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(180.))),
                modifiers: gpui::Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
            scroll_momentum_tests::settle(vcx);
            if let (Some(pinned), Some(caption)) = (
                vcx.debug_bounds("pinned-latest-prompt"),
                vcx.debug_bounds("user-prompt-451"),
            ) {
                // While the next card pushes this one out it is deliberately
                // shifted up and clipped. Judge only settled, unclipped frames.
                if panel.read_with(vcx, |p, _| p.offscreen_prompt_clip.is_some()) {
                    continue;
                }
                assert!(
                    caption.top() >= pinned.top() && caption.bottom() <= pinned.bottom(),
                    "caption {caption:?} pinned {pinned:?}"
                );
                panel.read_with(vcx, |panel, _| {
                    assert_eq!(
                        panel.offscreen_prompt,
                        Some(451),
                        "newest prompt must not replace historical context"
                    );
                });
                if let Some(newer) = vcx.debug_bounds("user-prompt-492") {
                    assert!(
                        newer.top() > pinned.bottom(),
                        "a newer inline prompt can enter below the historical sticky card"
                    );
                }
                found_historical = true;
                break;
            }
        }
        assert!(
            found_historical,
            "native upward scrolling must paint the twelfth prompt"
        );
        println!("ACCEPTANCE: native upward scrolling replaces pinned prompt 13 with prompt 12");
        panel.update(vcx, |panel, cx| {
            assert!(!panel.stick_to_bottom);
            panel
                .items
                .push(Item::User("A newer prompt arrives".into()));
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("user-prompt-451").is_some());
        assert!(vcx.debug_bounds("user-prompt-533").is_none());
        println!("ACCEPTANCE: arrival of prompt 14 leaves viewed prompt 12 pinned");
        // Travel to each end through the real wheel handler. Only the end
        // state is asserted, so long strokes cover 500+ rows in a few steps
        // instead of hundreds of small, separately settled ones.
        for (delta, label) in [(2400., "user-prompt-0"), (-2400., "user-prompt-533")] {
            for _ in 0..250 {
                let position = vcx.debug_bounds("transcript").unwrap().center();
                vcx.simulate_event(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
                    modifiers: gpui::Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                });
                vcx.run_until_parked();
                scroll_momentum_tests::settle(vcx);
                if vcx.debug_bounds(label).is_some()
                    && vcx.debug_bounds("pinned-latest-prompt").is_none()
                {
                    break;
                }
            }
            assert!(
                vcx.debug_bounds(label).is_some(),
                "transcript card {label} paints"
            );
            assert!(
                vcx.debug_bounds("pinned-latest-prompt").is_none(),
                "visible user card must not be duplicated"
            );
        }
        println!(
            "ACCEPTANCE: native top/bottom scrolling paints prompt 1 and prompt 14 without duplicate pinned cards"
        );
    }

    #[gpui::test]
    fn scrolling_history_pins_the_historical_turn(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "historical-prompts".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            for turn in 0..3 {
                panel.items.push(Item::User(format!("Prompt {turn}")));
                for line in 0..40 {
                    panel
                        .items
                        .push(Item::Assistant(format!("Turn {turn} response {line}")));
                }
            }
            cx.notify();
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, _| assert_eq!(panel.offscreen_prompt, Some(82)));
        for (row, expected) in [(50, Some(41)), (10, Some(0)), (0, None)] {
            panel.update(vcx, |panel, cx| {
                panel.stick_to_bottom = false;
                panel.transcript_list.scroll_to(gpui::ListOffset {
                    item_ix: row,
                    offset_in_item: px(0.),
                });
                cx.notify();
            });
            vcx.run_until_parked();
            panel.update(vcx, |panel, _| assert_eq!(panel.offscreen_prompt, expected));
        }
    }

    #[gpui::test]
    fn pinned_prompt_transition_does_not_move_the_transcript(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "prompt-transition".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(500.)));
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::User("First prompt".into()));
            for n in 0..40 {
                panel
                    .items
                    .push(Item::Assistant(format!("Response paragraph {n}")));
            }
            cx.notify();
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = false;
            panel.transcript_list.scroll_to(gpui::ListOffset::default());
            cx.notify();
        });
        vcx.run_until_parked();
        let viewport = vcx.debug_bounds("transcript").unwrap();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());

        for (delta, expected_pin) in [(-4., true), (4., false)] {
            let mut crossed = false;
            for _ in 0..80 {
                let before = vcx.debug_bounds("transcript-row-1").unwrap().top();
                vcx.simulate_event(gpui::ScrollWheelEvent {
                    position: viewport.center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
                    modifiers: Default::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                });
                vcx.run_until_parked();
                scroll_momentum_tests::settle(vcx);
                assert_eq!(
                    vcx.debug_bounds("transcript").unwrap(),
                    viewport,
                    "pinning must not resize or move the scroll viewport"
                );
                let after = vcx.debug_bounds("transcript-row-1").unwrap().top();
                assert!(
                    (f32::from(after - before) - delta).abs() < 0.5,
                    "text must move only by the scroll delta, not the header height"
                );
                if vcx.debug_bounds("pinned-latest-prompt").is_some() == expected_pin {
                    crossed = true;
                    break;
                }
            }
            assert!(crossed, "native scrolling must cross the pin boundary");
            for _ in 0..4 {
                panel.update(vcx, |_, cx| cx.notify());
                vcx.run_until_parked();
                assert_eq!(vcx.debug_bounds("transcript").unwrap(), viewport);
                assert_eq!(
                    vcx.debug_bounds("pinned-latest-prompt").is_some(),
                    expected_pin
                );
            }
        }
    }

    #[gpui::test]
    fn pinned_prompt_tracks_scrolling_and_new_prompts(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "prompt-visibility".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::User("Keep the normal **prompt card**".into())];
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());

        // With no todos, a long response still pins the offscreen prompt.
        panel.update(vcx, |panel, cx| {
            for i in 0..40 {
                panel
                    .items
                    .push(Item::Assistant(format!("Response paragraph {i}")));
            }
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_some());

        // A partially scrolled original remains sticky once its card top crosses.
        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = false;
            panel.transcript_list.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(15.),
            });
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("transcript-row-0").is_some());
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_some());

        panel.update(vcx, |panel, cx| {
            panel.stick_to_bottom = true;
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_some());

        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::User("A new visible prompt".into()));
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());
    }

    #[gpui::test]
    fn pinned_prompt_card_is_below_todos_and_resize_aware(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "prompt-todos".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(400.)));
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![
                Item::User("Keep the prompt below the tasks".into()),
                Item::Todos(serde_json::from_value(serde_json::json!({
                    "todos": [{"id":"one", "content":"Verify visibility", "status":"in_progress", "priority":"high"}]
                })).unwrap()),
            ];
            for i in 0..20 {
                panel.items.push(Item::Assistant(format!("Response paragraph {i}")));
            }
            cx.notify();
        });
        vcx.run_until_parked();
        let todos = vcx.debug_bounds("pinned-todo-card").unwrap();
        let prompt = vcx.debug_bounds("pinned-latest-prompt").unwrap();
        let transcript = vcx.debug_bounds("transcript").unwrap();
        assert_eq!(
            prompt.top() - todos.bottom(),
            px(8.),
            "todo breathing room belongs above the whole viewport"
        );
        assert_eq!(prompt.top(), transcript.top());
        assert!(prompt.bottom() < transcript.bottom());
        let fade = vcx.debug_bounds("pinned-prompt-fade").unwrap();
        assert_eq!(
            fade.top(),
            prompt.bottom(),
            "fade must meet the opaque backdrop without a seam"
        );
        assert_eq!(fade.size.width, prompt.size.width);
        assert_eq!(fade.size.height, px(18.));

        // Workspace selection, not keyboard focus, chooses the reading surface.
        // Switching it must invalidate cached rendering without moving the card.
        for focused in [false, true] {
            panel.update(vcx, |panel, cx| panel.set_surface_focused(focused, cx));
            vcx.run_until_parked();
            panel.update(vcx, |panel, _| assert_eq!(panel.surface_focused, focused));
            assert_eq!(vcx.debug_bounds("pinned-latest-prompt").unwrap(), prompt);
            assert_eq!(vcx.debug_bounds("pinned-prompt-fade").unwrap(), fade);
        }

        panel.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        let todos = vcx.debug_bounds("pinned-todo-card").unwrap();
        let prompt = vcx.debug_bounds("pinned-latest-prompt").unwrap();
        assert_eq!(
            prompt.top() - todos.bottom(),
            px(8.),
            "expanded todo has the same gap"
        );
        assert_eq!(prompt.top(), vcx.debug_bounds("transcript").unwrap().top());

        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(1600.)));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());
        assert!(vcx.debug_bounds("pinned-todo-card").is_some());
        assert!(vcx.debug_bounds("pinned-prompt-fade").is_none());
        panel.update(vcx, |panel, cx| {
            panel.items.retain(|item| !matches!(item, Item::Todos(_)));
            cx.notify();
        });
        vcx.simulate_window_resize(handle, gpui::size(px(600.), px(400.)));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-todo-card").is_none());
        let prompt = vcx.debug_bounds("pinned-latest-prompt").unwrap();
        assert_eq!(
            prompt.top(),
            px(8.),
            "without a pinned todo the transcript starts at the normal panel inset"
        );
        assert_eq!(prompt.top(), vcx.debug_bounds("transcript").unwrap().top());
    }
}
