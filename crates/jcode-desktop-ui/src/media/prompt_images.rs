//! The image chips that stand in for `[#image N]` markers inside rendered
//! prompt text, shared by prose and code blocks.
//!
//! A document render is synchronous, so the images ride in a scoped
//! thread-local: [`with_prompt_images`] installs them for one render and the
//! text leaves ask for them with [`current`].
use std::cell::RefCell;
use std::sync::Arc;

use crate::text_selection;

thread_local! {
    static CURRENT: RefCell<Option<PromptImages>> = const { RefCell::new(None) };
}

/// The images installed for the render in progress, if any.
pub(crate) fn current() -> Option<PromptImages> {
    CURRENT.with(|slot| slot.borrow().clone())
}

#[derive(Clone)]
pub(crate) struct PromptImages {
    pub attachments: std::rc::Rc<std::collections::HashMap<String, PromptImage>>,
    pub on_click: crate::image_chip::PromptImageClick,
}

pub(crate) struct PromptImage {
    pub slot: usize,
    pub label: String,
    pub preview: Option<Arc<gpui::Image>>,
}

pub(crate) type ImageChip = (std::ops::Range<usize>, String, Option<Arc<gpui::Image>>);

pub(crate) fn project_prompt_images(
    text: &str,
    prefix: &str,
    markers: &[(std::ops::Range<usize>, &PromptImage)],
    font: gpui::Font,
    font_size: gpui::Pixels,
    window: &gpui::Window,
) -> (String, text_selection::LayoutOffsets, Vec<ImageChip>) {
    let mut display = prefix.to_owned();
    let mut replacements = Vec::with_capacity(markers.len());
    let mut chips = Vec::with_capacity(markers.len());
    let mut offset = 0;
    for (range, image) in markers {
        display.push_str(&text[offset..range.start]);
        let start = display.len();
        if !display.is_empty() && !display.ends_with([' ', '\n']) {
            display.push(' ');
        }
        let chip_start = display.len();
        display.push_str(&crate::image_chip::transcript_spacer(
            &image.label,
            font.clone(),
            font_size,
            window,
        ));
        chips.push((
            chip_start..display.len(),
            image.label.clone(),
            image.preview.clone(),
        ));
        let next = &text[range.end..];
        if !next.is_empty() && !next.starts_with([' ', '\n']) {
            display.push(' ');
        }
        replacements.push((range.clone(), start..display.len()));
        offset = range.end;
    }
    display.push_str(&text[offset..]);
    (
        display,
        text_selection::LayoutOffsets::new(prefix.len(), replacements),
        chips,
    )
}

#[cfg(test)]
pub(crate) fn take_painted_prompt_thumbnails() -> Vec<gpui::Bounds<gpui::Pixels>> {
    crate::image_chip::take_painted_thumbnails()
}

/// Scope marker callbacks to this synchronous document render, restoring even
/// when rendering panics or nests another prompt.
pub(crate) fn with_prompt_images<R>(images: PromptImages, render: impl FnOnce() -> R) -> R {
    struct Restore(Option<PromptImages>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let previous = CURRENT.with(|slot| slot.replace(Some(images)));
    let _restore = Restore(previous);
    render()
}
