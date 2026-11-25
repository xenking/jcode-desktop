//! Session-local image browser. Images retain their transcript anchors, while
//! pane mode replaces large inline previews with compact navigation links.
use super::*;

#[derive(Clone, serde::Deserialize)]
struct GalleryImage {
    image_id: String,
    #[serde(default)]
    ordinal: Option<usize>,
    message_index: usize,
    timestamp: Option<String>,
    media_type: String,
}

impl GalleryImage {
    fn transcript_image(&self, data: String) -> TranscriptImage {
        let mut image = TranscriptImage::new(self.media_type.clone(), data, Some(format!("[#{}]", self.image_id)));
        image.session_image = self.ordinal.map(|ordinal| jcode_sdk::api::SessionImageRef {
            image_id: self.image_id.clone(), ordinal,
        });
        image
    }
}

#[derive(Default)]
pub(super) struct Gallery {
    metadata: Option<Result<Vec<GalleryImage>, String>>,
    live_ordinals: HashMap<String, usize>,
    requested_prompt_keys: std::collections::HashSet<String>,
    loading: bool,
}

impl Gallery {
    pub(super) fn receive(&mut self, result: Result<String, String>) {
        self.loading = false;
        self.metadata = Some(result.and_then(|json| {
            serde_json::from_str(&json).map_err(|error| error.to_string())
        }));
    }

    pub(super) fn loaded_image(&self, image_id: &str, data: &str) -> Option<TranscriptImage> {
        let image = self.metadata.as_ref()?
            .as_ref()
            .ok()?
            .iter()
            .find(|image| image.image_id == image_id)?;
        Some(image.transcript_image(data.to_owned()))
    }

    pub(super) fn highest_ordinal(&self) -> Option<usize> {
        self.metadata.as_ref()?.as_ref().ok()?.iter().filter_map(|image| image.ordinal).max()
    }

    fn ordinal(&self, id: &str) -> Option<usize> {
        self.live_ordinals
            .get(id)
            .copied()
            .or_else(|| {
                self.metadata
                    .as_ref()?
                    .as_ref()
                    .ok()?
                    .iter()
                    .find(|image| image.image_id == id)?
                    .ordinal
            })
            .filter(|ordinal| *ordinal > 0)
    }
}

impl Panel {
    pub(super) fn note_session_image_ordinal(&mut self, ordinal: Option<usize>, cx: &mut Context<Self>) {
        if let Some(ordinal) = ordinal.filter(|&ordinal| ordinal > self.session_image_base) {
            self.session_image_base = ordinal;
            self.input.update(cx, |input, cx| input.set_session_image_base(ordinal, cx));
        }
    }

    pub(super) fn observe_session_image_ids(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        let ApiEvent::UserImageIds { session_id, original_content, content, image_ids, image_ordinals } = event else { return };
        if session_id != &self.session_id { return; }
        self.note_session_image_ordinal(image_ordinals.iter().copied().max(), cx);
        self.session_image_gallery.live_ordinals.extend(
            image_ids
                .iter()
                .cloned()
                .zip(image_ordinals.iter().copied())
                .filter(|(_, ordinal)| *ordinal > 0),
        );
        let Some(index) = self.items.iter().enumerate().find_map(|(index, item)| {
            matches!(item, Item::User(text) if text == original_content && !self.user_attachment_range(index).is_empty()).then_some(index)
        }).or_else(|| self.items.iter().rposition(|item| matches!(item, Item::User(text) if text == original_content))) else { return };
        self.items[index] = Item::User(content.clone());
        for (position, (slot, image_id)) in
            self.user_attachment_range(index).zip(image_ids).enumerate()
        {
            if let Item::Image(image) = &mut self.items[slot] {
                image.remember_image_id(image_id);
                if let Some(&ordinal) = image_ordinals.get(position).filter(|&&ordinal| ordinal > 0)
                {
                    image.session_image = Some(jcode_sdk::api::SessionImageRef {
                        image_id: image_id.clone(),
                        ordinal,
                    });
                }
                image.provisional_image_ordinal = None;
            }
        }
        self.transcript_measurements.dirty = true;
    }

    pub(super) fn reconcile_history_images(
        &mut self,
        messages: &[jcode_sdk::HistoryMessage],
        images: Vec<jcode_sdk::RenderedImage>,
    ) {
        let prompts: Vec<_> = messages.iter().filter(|message| message.role == "user").collect();
        let mut next_slot = HashMap::<usize, usize>::new();
        for image in images {
            if !matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
                self.insert_rendered_image(image);
                continue;
            }
            let Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal }) = image.anchor else {
                continue;
            };
            let Some(prompt) = prompts.get(ordinal) else {
                continue;
            };
            // The ordinal belongs to the returned window, not the live panel.
            // Timestamps distinguish repeated prompts; otherwise merge only an
            // unambiguous text match (normally including stable image IDs).
            let mut matches = self.items.iter().enumerate().filter_map(|(index, item)| {
                let Item::User(text) = item else { return None };
                (text == &prompt.content && prompt::is_pinnable_prompt(item)).then_some(index)
            });
            let Some(first) = matches.next() else { continue };
            let owner = if let Some(second) = matches.next() {
                let mut timed = [first, second].into_iter().chain(matches).filter(|index| {
                    self.user_times.get(index).copied().is_some_and(|at| Some(at) == prompt.timestamp_ms)
                });
                let Some(owner) = timed.next() else { continue };
                if timed.next().is_some() {
                    continue;
                }
                owner
            } else {
                first
            };
            let range = self.user_attachment_range(owner);
            let offset = next_slot.entry(ordinal).or_default();
            let index = range.start + *offset;
            *offset += 1;
            if index < range.end {
                let Item::Image(existing) = &mut self.items[index] else { unreachable!() };
                let existing_lazy = existing.data.starts_with("jcode-lazy:");
                let incoming_lazy = image.data.starts_with("jcode-lazy:");
                if existing.data != image.data && !(incoming_lazy && !existing_lazy && existing.preview.is_some()) {
                    *existing = TranscriptImage::from_rendered(image);
                } else if let Some(reference) = image.session_image {
                    existing.session_image = Some(reference);
                    existing.provisional_image_ordinal = None;
                }
            } else {
                self.insert_transcript_image(range.end, TranscriptImage::from_rendered(image));
            }
        }
    }

    pub(super) fn user_attachment_range(&self, user_index: usize) -> std::ops::Range<usize> {
        let start = user_index.saturating_add(1).min(self.items.len());
        let count = self.items[start..].iter().take_while(|item| {
            matches!(item, Item::Image(image) if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput))
        }).count();
        start..start + count
    }

    /// Match the submitted markers and user images in their original order.
    /// Images with no marker retain the fallback chip for older histories.
    pub(super) fn user_image_markers(&self, user_index: usize) -> Vec<(usize, String)> {
        let Some(Item::User(text)) = self.items.get(user_index) else {
            return Vec::new();
        };
        crate::input::image_marker_ranges(text).into_iter().enumerate().filter_map(|(slot, range)| {
            let marker = &text[range];
            let index = self.prompt_image_index(user_index, slot, marker)?;
            self.user_attachment_range(user_index).contains(&index).then(|| (index, marker.to_owned()))
        }).collect()
    }

    pub(super) fn prompt_image_index(&self, user_index: usize, slot: usize, marker: &str) -> Option<usize> {
        let owned = self.user_attachment_range(user_index);
        let Some(id) = crate::image_chip::session_image_key(marker) else {
            return owned.into_iter().nth(slot);
        };
        let matches = |item: &Item| matches!(item, Item::Image(image) if image.has_image_id(id));
        owned
            .into_iter()
            .find(|index| matches(&self.items[*index]))
            .or_else(|| self.items.iter().position(matches))
    }

    pub(super) fn gallery_image_ordinal(&self, marker: &str) -> Option<usize> {
        self.session_image_gallery
            .ordinal(crate::image_chip::session_image_key(marker)?)
    }

    pub(super) fn render_user_image_chip(
        &self,
        index: usize,
        image: &TranscriptImage,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let label = image.image_ordinal().map_or_else(
            || crate::image_chip::UNKNOWN_LABEL.to_owned(),
            crate::image_chip::numbered_label,
        );
        if image.preview.is_none() {
            self.request_history_thumbnail(image, false);
        }
        let preview = image.clone();
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .id(("transcript-user-image-chip", index))
                    .debug_selector(|| "transcript-user-image-chip".into())
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded_full()
                    .px_2()
                    .py_1()
                    .text_size(px(11.))
                    .text_color(Theme::global().TEXT_DIM)
                    .bg(Theme::global().USER_BG)
                    .cursor_pointer()
                    .hover(|style| style.text_color(Theme::global().TEXT))
                    .children(image.preview.clone().map(|image| {
                        img(crate::image_cache::source(image))
                            .debug_selector(|| "transcript-fallback-thumbnail".into())
                            .size(px(14.))
                            .rounded_sm()
                            .object_fit(gpui::ObjectFit::Cover)
                    }))
                    .child(label)
                    .when(image.preview.is_none(), |chip| {
                        chip.child(div().debug_selector(|| "image-chip-placeholder".into())
                            .child("No preview"))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_image_preview(preview.clone(), window, cx);
                        cx.stop_propagation();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn session_image_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| matches!(item, Item::Image(_)).then_some(index))
            .collect()
    }

    pub(super) fn selected_session_image_index(&self) -> Option<usize> {
        self.image_pane_selected
            .filter(|index| matches!(self.items.get(*index), Some(Item::Image(_))))
            .or_else(|| {
                self.items
                    .iter()
                    .rposition(|item| matches!(item, Item::Image(_)))
            })
    }

    pub(super) fn set_image_pane_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.image_pane_open = open;
        if open {
            self.request_session_image_gallery();
        } else {
            self.pending_history_image = None;
        }
        self.cancel_transcript_momentum();
        self.transcript_measurements.dirty = true;
        cx.notify();
    }

    fn select_session_image(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.image_pane_selected = index;
        self.reveal_selected_session_image();
        cx.notify();
    }

    fn reveal_selected_session_image(&self) {
        if let Some(index) = self.selected_session_image_index() {
            let ordinal = self.items[..index]
                .iter()
                .filter(|item| matches!(item, Item::Image(_)))
                .count();
            self.image_pane_scroll.scroll_to_item(ordinal);
        }
    }

    fn image_pane_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }
        let step = match event.keystroke.key.as_str() {
            "left" => -1,
            "right" => 1,
            _ => return,
        };
        let indices = self.session_image_indices();
        if let Some(selected) = self.selected_session_image_index()
            && let Ok(ordinal) = indices.binary_search(&selected)
        {
            let next = ordinal.saturating_add_signed(step).min(indices.len() - 1);
            self.select_session_image(Some(indices[next]), cx);
        }
        cx.stop_propagation();
    }

    fn render_fitted_gallery_image(
        &self,
        image: &TranscriptImage,
        preview: std::sync::Arc<gpui::Image>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let preview_image = image.clone();
        let marker = crate::fixture_control_targets::marker(
            || "gallery-nav:picture".into(),
            || Some(self.session_id.clone()),
        );
        let open_preview = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.open_image_preview(preview_image.clone(), window, cx);
            cx.stop_propagation();
        });
        gpui::canvas(
            move |bounds, window, cx| {
                let (mut picture, fitted) = match crate::image_cache::load(&preview, window, cx) {
                    Some(Ok(decoded)) => {
                        let fitted = gpui::ObjectFit::Contain.get_bounds(bounds, decoded.size(0));
                        let picture = div()
                            .id("session-image-picture")
                            .debug_selector(|| "session-image-picture".into())
                            .relative()
                            .children(marker)
                            .size_full()
                            .cursor_pointer()
                            .child(
                                img(decoded)
                                    .debug_selector(|| "session-image-preview-ready".into())
                                    .size_full(),
                            )
                            .on_click(open_preview)
                            .into_any_element();
                        (picture, fitted)
                    }
                    state => (image_pane_placeholder(state.is_some()), bounds),
                };
                picture.layout_as_root(fitted.size.map(gpui::AvailableSpace::Definite), window, cx);
                picture.prepaint_at(fitted.origin, window, cx);
                picture
            },
            |_, mut picture, window, cx| picture.paint(window, cx),
        )
        .absolute()
        .size_full()
    }

    pub(super) fn request_session_image_gallery(&mut self) {
        if self.session_image_gallery.loading || self.is_pending_session() {
            return;
        }
        self.session_image_gallery.loading = true;
        self.bridge.send(Command::ListSessionImages {
            session_id: self.session_id.clone(),
        });
    }

    pub(super) fn may_request_prompt_image_ordinal(&self, key: &str) -> bool {
        !self.is_pending_session()
            && !self
                .session_image_gallery
                .requested_prompt_keys
                .contains(key)
    }

    pub(super) fn request_prompt_image_ordinals(&mut self, unresolved: Vec<String>) {
        if self.is_pending_session() {
            return;
        }
        let mut new_key = false;
        for key in unresolved {
            new_key |= self.session_image_gallery.requested_prompt_keys.insert(key);
        }
        if new_key {
            self.request_session_image_gallery();
        }
    }

    pub(super) fn render_image_pane_toggle(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.draft_has_images
            && self.selected_session_image_index().is_none()
            && !self.session_image_gallery.metadata.as_ref().is_some_and(|result| {
                result.as_ref().is_ok_and(|images| !images.is_empty())
            })
        {
            return None;
        }
        let theme = Theme::global();
        let color = if self.image_pane_open {
            theme.TEXT
        } else {
            theme.TEXT_DIM
        };
        div()
            .id("panel-images")
            .debug_selector(|| "panel-images".into())
            .relative()
            .children(crate::fixture_control_targets::marker(
                || "composer:images".into(),
                || Some(self.session_id.clone()),
            ))
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(self.image_pane_open, |el| el.bg(theme.ACCENT_DIM))
            .hover(|el| el.bg(theme.QUOTE_BG))
            .tooltip(|_, cx| {
                cx.new(|_| super::usage::MeterTooltip("Session images".into()))
                    .into()
            })
            .child(
                gpui::svg()
                    .data(include_bytes!("../../../assets/icons/image.svg") as &'static [u8])
                    .text_color(color)
                    .size(px(13.)),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.set_image_pane_open(!this.image_pane_open, cx);
                if !this.image_pane_open && this.image_pane_focus.is_focused(window) {
                    this.focus_input(window, cx);
                }
                cx.stop_propagation();
            }))
            .into_any_element()
            .into()
    }

    pub(super) fn render_image_pane_link(
        &self,
        index: usize,
        image: &TranscriptImage,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let label = image.display_label("Image attachment");
        div()
            .id(("transcript-image-link", index))
            .debug_selector(move || format!("transcript-image-link-{index}"))
            .px_2()
            .py_1()
            .rounded_md()
            .text_size(px(11.))
            .text_color(Theme::global().TEXT_DIM)
            .bg(Theme::global().USER_BG)
            .cursor_pointer()
            .hover(|el| el.text_color(Theme::global().TEXT))
            .child(format!(
                "{} · {label} · View in Images ↗",
                image.model_input_caption().unwrap_or("Image attachment")
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_session_image(Some(index), cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn render_image_pane(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.image_pane_open {
            return None;
        }
        let theme = Theme::global();
        let indices = self.session_image_indices();
        let selected = self.selected_session_image_index();
        let gallery = self.session_image_gallery.metadata.clone();
        let gallery_has_images = gallery
            .as_ref()
            .is_some_and(|result| result.as_ref().is_ok_and(|images| !images.is_empty()));
        let image = selected.and_then(|index| match &self.items[index] {
            Item::Image(image) => Some(image),
            _ => None,
        });
        let pane = div()
            .id("session-image-pane")
            .debug_selector(|| "session-image-pane".into())
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .min_h_0()
            .when(self.image_pane_stacked, |el| {
                el.w_full().h(gpui::relative(0.45)).border_t_1()
            })
            .when(!self.image_pane_stacked, |el| {
                el.w(gpui::relative(0.4)).h_full().border_l_1()
            })
            .border_color(theme.PANEL_BORDER)
            .bg(theme.PANEL_BG)
            .p_3()
            .gap(if self.image_pane_stacked { px(4.) } else { px(8.) })
            .overflow_hidden()
            // The workspace handles gestures in capture, before a scrolling Div.
            // Exclude its hitbox here even when the gallery is already at an edge.
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(theme.TEXT)
                            .child(format!(
                                "Session images · {}",
                                gallery
                                    .as_ref()
                                    .and_then(|result| result.as_ref().ok())
                                    .map_or(indices.len(), Vec::len)
                            )),
                    )
                    .when(!indices.is_empty(), |header| {
                        header.child(
                            div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(theme.TEXT_DIM)
                                .child("←/→ Browse · Esc Close"),
                        )
                    })
                    .child(
                        div()
                            .id("session-image-close")
                            .debug_selector(|| "session-image-close".into())
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .cursor_pointer()
                            .hover(|el| el.bg(theme.QUOTE_BG))
                            .child("Inline ×")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_image_pane_open(false, cx);
                                this.focus_input(window, cx);
                                cx.stop_propagation();
                            })),
                    ),
            )
            .when_some(gallery, |el, gallery| match gallery {
                Ok(images) => el.child(
                    div()
                        .id("session-gallery-list")
                        .debug_selector(|| "session-gallery-list".into())
                        .flex_none()
                        .when(self.image_pane_stacked, |list| list.max_h(px(36.)))
                        .when(!self.image_pane_stacked, |list| list.max_h(px(140.)))
                        .w_full()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(images.into_iter().enumerate().map(|(index, image)| {
                            let id = image.image_id.clone();
                            let image_label = image.ordinal.map_or_else(|| id.clone(), |ordinal| format!("Image #{ordinal}"));
                            let label = format!(
                                "{} · message {} · {}",
                                image_label,
                                image.message_index + 1,
                                image.timestamp.as_deref().unwrap_or("time unavailable")
                            );
                            div()
                                .id(("session-gallery-image", index))
                                .debug_selector(move || format!("session-gallery-image-{index}"))
                                .flex_none()
                                .when(index == 0, |row| {
                                    row.relative().children(crate::fixture_control_targets::marker(
                                        || "gallery-nav:history".into(),
                                        || Some(self.session_id.clone()),
                                    ))
                                })
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(theme.TEXT_DIM)
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.QUOTE_BG))
                                .child(label)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let image = image.transcript_image(format!("jcode-lazy:{id}"));
                                    this.open_image_preview(image, window, cx);
                                    cx.stop_propagation();
                                }))
                        })),
                ),
                Err(error) => el.child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(format!("Could not load session images: {error}")),
                ),
            })
            .when_some(image, |el, image| {
                let asset = self.gallery_image_asset(image);
                el.child(
                    div()
                        .id("session-image-main")
                        .debug_selector(|| "session-image-main".into())
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .rounded_md()
                        .overflow_hidden()
                        .bg(theme.USER_BG)
                        .children(self.gallery_image_preloader(image, cx))
                        .children(
                            crate::fixture_control_targets::marker(
                                || "gallery-nav:gap".into(),
                                || Some(self.session_id.clone()),
                            )
                            .map(|marker| marker.size(px(2.))),
                        )
                        .when_some(asset.clone(), |el, preview| {
                            el.child(self.render_fitted_gallery_image(image, preview, cx))
                        })
                        .when(asset.is_none(), |el| {
                            el.child(image_pane_placeholder(self.gallery_image_unavailable(image)))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(theme.TEXT_FAINT)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(theme.TEXT_DIM)
                                .child(format!(
                                    "{} · {}",
                                    image.display_label("Image attachment"),
                                    image.model_input_caption().unwrap_or("Session attachment")
                                )),
                        )
                        .child(
                            div()
                                .id("session-image-latest")
                                .debug_selector(|| "session-image-latest".into())
                                .flex_none()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.QUOTE_BG))
                                .child(if self.image_pane_selected.is_none() {
                                    "Following latest"
                                } else {
                                    "Follow latest"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.select_session_image(None, cx);
                                    cx.stop_propagation();
                                })),
                        ),
                )
            })
            .when(image.is_none(), |el| {
                el.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(theme.TEXT_DIM)
                        .child(if gallery_has_images {
                            "Choose an image above"
                        } else {
                            "No images in the loaded transcript"
                        })
                        .child(if gallery_has_images {
                            "Images from older messages can be opened by ID."
                        } else {
                            "Session images appear here when available."
                        }),
                )
            })
            .when(!indices.is_empty(), |el| {
                let strip = div()
                        .id("session-image-thumbnails")
                        .debug_selector(|| "session-image-thumbnails".into())
                        .track_focus(&self.image_pane_focus)
                        .key_context(crate::menu::CONTEXT)
                        .on_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                            this.set_image_pane_open(false, cx);
                            this.focus_input(window, cx);
                        }))
                        .on_key_down(cx.listener(Self::image_pane_key_down))
                        .flex_none()
                        .h(px(70.))
                        .w_full()
                        .p(px(2.))
                        .rounded_md()
                        .border_1()
                        .border_color(theme.PANEL_BORDER)
                        .focus(|style| style.border_color(Theme::global().ACCENT))
                        .overflow_x_scroll()
                        .track_scroll(&self.image_pane_scroll)
                        .flex()
                        .gap_2()
                        .children(indices.into_iter().enumerate().map(|(number, index)| {
                            let Item::Image(image) = &self.items[index] else {
                                unreachable!()
                            };
                            let asset = self.gallery_image_asset(image);
                            div()
                                .id(("session-image-thumb", index))
                                .debug_selector(move || format!("session-image-thumb-{index}"))
                                .relative()
                                .children(crate::fixture_control_targets::marker(
                                    || format!("gallery:thumb:{number}"),
                                    || Some(self.session_id.clone()),
                                ))
                                .flex_none()
                                .w(px(80.))
                                .h(px(64.))
                                .rounded_md()
                                .overflow_hidden()
                                .border_1()
                                .border_color(if selected == Some(index) {
                                    theme.ACCENT
                                } else {
                                    theme.PANEL_BORDER
                                })
                                .bg(theme.USER_BG)
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.QUOTE_BG))
                                .children(self.gallery_image_preloader(image, cx))
                                .when_some(asset.clone(), |el, preview| {
                                    el.child(
                                        img(crate::image_cache::source(preview))
                                            .debug_selector(move || format!("session-image-thumb-ready-{index}"))
                                            .size_full()
                                            .object_fit(gpui::ObjectFit::Contain)
                                            .with_fallback(|| image_pane_placeholder(true)),
                                    )
                                })
                                .when(asset.is_none(), |el| {
                                    el.flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(11.))
                                        .text_color(theme.TEXT_DIM)
                                        .child(if self.gallery_image_unavailable(image) {
                                            "Unavailable".to_string()
                                        } else {
                                            format!("Image {}", number + 1)
                                        })
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_session_image(Some(index), cx);
                                    cx.stop_propagation();
                                }))
                        }));
                el.child(
                    div()
                        .relative()
                        .flex_none()
                        .children(crate::fixture_control_targets::marker(
                            || "gallery:filmstrip".into(),
                            || Some(self.session_id.clone()),
                        ))
                        .child(strip),
                )
            });
        Some(pane.into_any_element())
    }

    /// Use this panel's bounds, not the window's: split workspaces can be narrow.
    pub(super) fn image_pane_layout_observer(&self, cx: &Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        let was_stacked = self.image_pane_stacked;
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, _, cx| {
                let stacked = f32::from(bounds.size.width) < 760.;
                if was_stacked != stacked {
                    let panel = panel.clone();
                    cx.defer(move |cx| {
                        let _ = panel.update(cx, |panel, cx| {
                            if panel.image_pane_stacked != stacked {
                                panel.image_pane_stacked = stacked;
                                cx.notify();
                            }
                        });
                    });
                }
            },
        )
        .absolute()
        .size_full()
    }
}

fn image_pane_placeholder(unavailable: bool) -> gpui::AnyElement {
    div()
        .id("session-image-placeholder")
        .debug_selector(move || {
            if unavailable {
                "session-image-unavailable".into()
            } else {
                "session-image-placeholder".into()
            }
        })
        .flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .text_color(Theme::global().TEXT_DIM)
        .child(if unavailable { "Unavailable" } else { "Loading image…" })
        .into_any_element()
}
