//! Markdown renderer for chat transcripts.
//!
//! Line-based and streaming-tolerant: headings, fenced code with lightweight
//! syntax highlighting, nested and task lists, tables, block quotes,
//! mathematical notation, links, and the common inline spans. Deliberately
//! self-contained, but shaped so a half-finished response still reads well.

use gpui::{
    FontWeight, HighlightStyle, InteractiveText, SharedString, StrikethroughStyle, StyledText,
    UnderlineStyle, div, prelude::*, px, relative,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};

use crate::text_selection::{self, TextSelection};
use crate::theme::{Palette, Theme, to_hsla};
use jcode_desktop_model::markdown_file::{self, FileReferenceCache, LinkTarget};

mod diagram;
mod latex;
mod parse;
#[cfg(test)]
mod tests;

use crate::code_block::{code_block_with_selection, flatten_highlights, hash};
use crate::prompt_images::{self, ImageChip, project_prompt_images};
use diagram::*;
use latex::*;
use parse::*;

pub(crate) use crate::prompt_images::{PromptImage, PromptImages, with_prompt_images};
pub(crate) use diagram::MediaPreviewHandler;
pub(crate) use parse::public_transcript;
#[cfg(test)]
pub(crate) use crate::prompt_images::take_painted_prompt_thumbnails;
#[cfg(test)]
pub(crate) use diagram::mermaid_natural_size;
#[cfg(test)]
pub(crate) use parse::parse_count;

type OpenHeading = dyn Fn(&str, &mut gpui::App);

pub(crate) struct FileLinkHandler {
    base: PathBuf,
    document: Option<(PathBuf, Box<OpenHeading>)>,
}

impl FileLinkHandler {
    pub(crate) fn for_transcript(base: PathBuf) -> Rc<Self> {
        Rc::new(Self {
            base,
            document: None,
        })
    }

    pub(crate) fn for_document(
        base: PathBuf,
        path: PathBuf,
        heading: impl Fn(&str, &mut gpui::App) + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            base,
            document: Some((path, Box::new(heading))),
        })
    }

    fn open(&self, target: LinkTarget, window: &mut gpui::Window, cx: &mut gpui::App) {
        match target {
            LinkTarget::External(url) => cx.open_url(&url),
            LinkTarget::Local { path, fragment } => {
                if let Some((current, heading)) = &self.document
                    && current == &path
                {
                    if let Some(fragment) = fragment {
                        heading(&fragment, cx);
                    }
                } else {
                    window.dispatch_action(
                        Box::new(crate::ui_actions::OpenDocumentFile { path, fragment }),
                        cx,
                    );
                }
            }
            LinkTarget::Anchor(fragment) => {
                if let Some((_, heading)) = &self.document {
                    heading(&fragment, cx);
                }
            }
        }
    }

    fn open_markdown(&self, destination: &str, window: &mut gpui::Window, cx: &mut gpui::App) {
        if let Some(target) = markdown_file::resolve(&self.base, destination) {
            self.open(target, window, cx);
        }
    }
}

pub(crate) type FileImageResolver = Rc<dyn Fn(&str) -> Option<std::path::PathBuf>>;

pub(crate) struct FileRenderOptions {
    pub scroll: gpui::ScrollHandle,
    pub on_link: Rc<FileLinkHandler>,
    pub resolve_image: FileImageResolver,
}

thread_local! {
    static FILE_OPTIONS: std::cell::RefCell<Option<Rc<FileRenderOptions>>> = const { std::cell::RefCell::new(None) };
    static TRANSCRIPT_LINKS: std::cell::RefCell<Option<(Rc<FileLinkHandler>, bool)>> = const { std::cell::RefCell::new(None) };
    static FILE_REFERENCES: std::cell::RefCell<FileReferenceCache> = Default::default();
}

pub(crate) fn with_file_link_handler<R>(
    handler: Rc<FileLinkHandler>,
    infer_references: bool,
    render: impl FnOnce() -> R,
) -> R {
    let previous = TRANSCRIPT_LINKS.with(|slot| slot.replace(Some((handler, infer_references))));
    let result = render();
    TRANSCRIPT_LINKS.with(|slot| slot.replace(previous));
    result
}

pub(crate) fn render_file(
    document: &FileDocument,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    options: FileRenderOptions,
) -> gpui::AnyElement {
    let previous = FILE_OPTIONS.with(|slot| slot.replace(Some(Rc::new(options))));
    let result = render_blocks(
        &document.0,
        0,
        "file",
        selection,
        window,
        cx,
        false,
        None,
        false,
        None,
    );
    FILE_OPTIONS.with(|slot| {
        slot.replace(previous);
    });
    result
}

pub(crate) struct FileDocument(Vec<Block>);

impl FileDocument {
    pub(crate) fn parse(source: &str) -> Self {
        Self(parse(source))
    }

    pub(crate) fn heading_index(&self, fragment: &str) -> Option<usize> {
        file_heading_index(&self.0, fragment)
    }
}

fn file_heading_index(blocks: &[Block], fragment: &str) -> Option<usize> {
    let mut used = std::collections::HashSet::new();
    for (index, block) in blocks.iter().enumerate() {
        if let Block::Heading(_, text) = block {
            let plain = inline_spans(text).plain;
            let base: String = plain
                .chars()
                .flat_map(char::to_lowercase)
                .filter_map(|c| {
                    if c.is_alphanumeric() || c == '_' || c == '-' {
                        Some(c)
                    } else if c.is_whitespace() {
                        Some('-')
                    } else {
                        None
                    }
                })
                .collect();
            let mut slug = base.clone();
            let mut suffix = 1;
            while !used.insert(slug.clone()) {
                slug = format!("{base}-{suffix}");
                suffix += 1;
            }
            if slug == fragment {
                return Some(index * 2);
            }
        }
    }
    None
}

fn file_image_parts(text: &str) -> Vec<(&str, Option<(&str, &str)>)> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let ticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            let fence = "`".repeat(ticks);
            if let Some(end) = text[i + ticks..].find(&fence) {
                i += ticks + end + ticks;
                continue;
            }
            i += ticks;
            continue;
        }
        if bytes[i..].starts_with(b"![") {
            if let Some(label_end) = text[i + 2..].find("](").map(|n| i + 2 + n) {
                let mut end = label_end + 2;
                let mut depth = 0usize;
                while end < bytes.len() {
                    match bytes[end] {
                        b'\\' if end + 1 < bytes.len() => {
                            end += 2;
                            continue;
                        }
                        b'(' => depth += 1,
                        b')' if depth == 0 => break,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if end < bytes.len() {
                    if start < i {
                        result.push((&text[start..i], None));
                    }
                    result.push((
                        "",
                        Some((&text[i + 2..label_end], text[label_end + 2..end].trim())),
                    ));
                    i = end + 1;
                    start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if start < text.len() {
        result.push((&text[start..], None));
    }
    result
}

struct FileImageAsset;

impl gpui::Asset for FileImageAsset {
    type Source = gpui::Resource;
    type Output = Result<Arc<gpui::RenderImage>, gpui::ImageCacheError>;

    fn load(
        source: Self::Source,
        cx: &mut gpui::App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let id = jcode_desktop_api::ImageIds::get(cx).next();
        let path = match &source {
            gpui::Resource::Path(path) => Some(path.clone()),
            _ => None,
        };
        let decode = <gpui::ImageAssetLoader as gpui::Asset>::load(source, cx);
        async move {
            if let Some(path) = path {
                let metadata = std::fs::metadata(path)?;
                if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
                    return Err(std::io::Error::other(
                        "image is not a regular file or is too large",
                    )
                    .into());
                }
            }
            let mut image = decode.await?;
            Arc::get_mut(&mut image).expect("fresh image decode").id = id;
            Ok(image)
        }
    }
}

#[derive(IntoElement)]
struct FileImage {
    key: SharedString,
    path: std::path::PathBuf,
    alt: String,
}

impl gpui::RenderOnce for FileImage {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let node = div().debug_selector(|| "markdown-file-image".into());
        match window.use_asset::<FileImageAsset>(&self.path.into(), cx) {
            Some(Ok(image)) if image.frame_count() > 0 => {
                let size = image.size(0).map(|dimension| u32::from(dimension) as f32);
                let scale = (540.0 / size.height).min(1.0);
                node.relative()
                    .flex_none()
                    .w(px(size.width * scale))
                    .max_w_full()
                    .aspect_ratio(size.width / size.height)
                    .child(
                        gpui::img(image)
                            .id(self.key)
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                    )
            }
            Some(_) => node.child(self.alt),
            None => node.text_color(Theme::global().TEXT_DIM).child(self.alt),
        }
    }
}

#[cfg(test)]
#[test]
fn file_heading_indices_follow_scroll_children_and_disambiguate() {
    let document =
        FileDocument::parse("# Hello, **world**!\n\nText\n\n## Hello, world!\n\n# Привет мир");
    assert_eq!(document.heading_index("hello-world"), Some(0));
    assert_eq!(document.heading_index("hello-world-1"), Some(4));
    assert_eq!(document.heading_index("привет-мир"), Some(6));
    assert_eq!(document.heading_index("missing"), None);
}

#[cfg(test)]
#[test]
fn file_images_keep_surrounding_prose() {
    assert_eq!(
        file_image_parts("before ![alt](image.png) after ![](https://example.com/a.png)"),
        vec![
            ("before ", None),
            ("", Some(("alt", "image.png"))),
            (" after ", None),
            ("", Some(("", "https://example.com/a.png"))),
        ]
    );
    assert_eq!(
        file_image_parts("`![no](x)` \\![no](x) ![yes](a(b).png)"),
        vec![
            ("`![no](x)` \\![no](x) ", None),
            ("", Some(("yes", "a(b).png")))
        ]
    );
}


/// A styled inline run. Links are clickable when any are present.
fn styled_line(
    source: &str,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    _window: &gpui::Window,
    cx: &gpui::App,
) -> gpui::AnyElement {
    styled_line_with_avatar(source, selection, key, _window, cx, false)
}

// A layout-only spacer participates in native shaping/wrapping, not paragraph
// padding: later visual lines start at the original left edge. Use ASCII
// spaces because GPUI's wrapper recognizes these as leading whitespace and
// does not create a word-break opportunity between them and the first word.
// Selection and clipboard offsets exclude the prefix entirely.
const AVATAR_PREFIX: &str = "    ";

fn assistant_avatar() -> impl IntoElement {
    gpui::svg()
        .debug_selector(|| "assistant-avatar".into())
        .data(crate::accounts::logo("jcode").expect("vendored Jcode logo"))
        .size(px(20.0))
        .text_color(Theme::global().TEXT)
}

fn styled_line_with_avatar(
    source: &str,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    _window: &gpui::Window,
    cx: &gpui::App,
    avatar: bool,
) -> gpui::AnyElement {
    if let Some(file) = FILE_OPTIONS
        .with(|slot| slot.borrow().clone())
        .filter(|_| source.contains("!["))
    {
        let parts = file_image_parts(source);
        if parts.iter().any(|(_, image)| image.is_some()) {
            return div()
                .flex()
                .flex_col()
                .children(
                    parts
                        .into_iter()
                        .enumerate()
                        .map(|(index, (prose, image))| {
                            let part_key: SharedString = format!("{key}-image-{index}").into();
                            if let Some((alt, target)) = image {
                                let target = target
                                    .strip_prefix('<')
                                    .and_then(|s| s.strip_suffix('>'))
                                    .unwrap_or(target);
                                let scheme = target.split_once(':').map(|(scheme, _)| scheme);
                                let external = scheme.is_some_and(|s| {
                                    s.eq_ignore_ascii_case("http")
                                        || s.eq_ignore_ascii_case("https")
                                });
                                if let Some(path) = (file.resolve_image)(target) {
                                    FileImage {
                                        key: part_key,
                                        path,
                                        alt: if alt.is_empty() { "Image" } else { alt }.to_owned(),
                                    }
                                    .into_any_element()
                                } else if external || target.starts_with("//") {
                                    let label =
                                        if alt.is_empty() { "Image" } else { alt }.to_owned();
                                    let target = if target.starts_with("//") {
                                        format!("https:{target}")
                                    } else {
                                        target.to_owned()
                                    };
                                    let on_link = file.on_link.clone();
                                    div()
                                        .id(part_key)
                                        .cursor_pointer()
                                        .text_color(Theme::global().ACCENT_MUTED)
                                        .on_click(move |_, window, cx| on_link.open_markdown(&target, window, cx))
                                        .child(label)
                                        .into_any_element()
                                } else {
                                    let label = if alt.is_empty() { "Image" } else { alt };
                                    styled_line_layout(
                                        &format!("{label} ({target})"),
                                        selection,
                                        part_key,
                                        _window,
                                        cx,
                                        false,
                                    )
                                    .0
                                }
                            } else {
                                styled_line_layout(
                                    prose,
                                    selection,
                                    part_key,
                                    _window,
                                    cx,
                                    avatar && index == 0,
                                )
                                .0
                            }
                        }),
                )
                .into_any_element();
        }
    }
    styled_line_layout(source, selection, key, _window, cx, avatar).0
}

/// The newest streamed block of a live row, fading in as one unit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct StreamFade {
    /// Trailing bytes of the markdown source in the fading block.
    pub(crate) bytes: usize,
    /// How transparent the block still is, 0 (opaque) to 1.
    pub(crate) amount: f32,
}

impl StreamFade {
    fn active(self) -> bool {
        self.bytes > 0 && self.amount > 0.0
    }

    fn with_bytes(self, bytes: usize) -> Self {
        Self { bytes, ..self }
    }
}

thread_local! {
    /// The fading block of the document being rendered. Set by the panel
    /// around a live row and spread over the trailing prose blocks, so a
    /// freshly released chunk of a streaming response eases into view.
    static STREAM_FADE: std::cell::Cell<StreamFade> =
        const { std::cell::Cell::new(StreamFade { bytes: 0, amount: 0.0 }) };
    /// The fade handed to the next inline text leaf.
    static LEAF_FADE: std::cell::Cell<StreamFade> =
        const { std::cell::Cell::new(StreamFade { bytes: 0, amount: 0.0 }) };
    /// Search terms to mark in the document being rendered: the picker's
    /// query, so the words that matched stand out in its preview and in the
    /// session it opened.
    static SEARCH_TERMS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    #[cfg(test)]
    static PAINTED_PROMPT_TEXT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    #[cfg(test)]
    static PAINTED_FONT_STYLES: std::cell::RefCell<Option<PaintedFontStyles>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
type PaintedFontStyles =
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, gpui::FontStyle>>>;

#[cfg(test)]
pub(crate) fn capture_painted_font_styles() -> PaintedFontStyles {
    let styles = PaintedFontStyles::default();
    PAINTED_FONT_STYLES.with(|slot| slot.replace(Some(styles.clone())));
    styles
}

#[cfg(test)]
pub(crate) fn take_rendered_prompt_text() -> Vec<String> {
    PAINTED_PROMPT_TEXT.with(|text| std::mem::take(&mut *text.borrow_mut()))
}


/// Render `f` with every occurrence of `terms` marked as a search match.
pub(crate) fn with_search_terms<R>(terms: &[String], render: impl FnOnce() -> R) -> R {
    if terms.is_empty() {
        return render();
    }
    struct Restore(Vec<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SEARCH_TERMS.with(|slot| *slot.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let previous = SEARCH_TERMS.with(|slot| slot.replace(terms.to_vec()));
    let _restore = Restore(previous);
    render()
}

/// The match style: a tinted background under the word, readable on both
/// prose and code, and distinct from the text selection colour.
pub(crate) fn search_match_style() -> HighlightStyle {
    let accent = Theme::global().ACCENT;
    HighlightStyle {
        background_color: Some(to_hsla(gpui::Rgba { a: 0.32, ..accent })),
        ..Default::default()
    }
}

fn search_highlights(text: &str) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    SEARCH_TERMS.with(|terms| {
        let terms = terms.borrow();
        crate::global_search::term_ranges(text, &terms)
            .into_iter()
            .map(|range| (range, search_match_style()))
            .collect()
    })
}

/// Render `f` with the trailing block of its markdown fading in.
pub(crate) fn with_stream_fade<R>(fade: StreamFade, f: impl FnOnce() -> R) -> R {
    STREAM_FADE.set(fade);
    let result = f();
    STREAM_FADE.set(StreamFade::default());
    LEAF_FADE.set(StreamFade::default());
    result
}

/// One uniform fade over the trailing `fade.bytes` of `text`. The whole block
/// shares one alpha, so it appears as a unit rather than letter by letter.
/// The range falls on a char boundary and leaves layout untouched.
pub(crate) fn fade_tail_highlights(
    text: &str,
    fade: StreamFade,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let bytes = fade.bytes.min(text.len());
    if bytes == 0 || fade.amount <= 0.0 {
        return Vec::new();
    }
    let start = text.floor_char_boundary(text.len() - bytes);
    vec![(
        start..text.len(),
        HighlightStyle {
            fade_out: Some(fade.amount.clamp(0.0, 1.0)),
            ..Default::default()
        },
    )]
}

/// Approximate source length of a block's prose, for spreading a fade.
fn block_prose_len(block: &Block) -> Option<usize> {
    match block {
        Block::Heading(_, text) | Block::Paragraph(text) => Some(text.len()),
        Block::Bullet { text, .. } | Block::Numbered { text, .. } => Some(text.len()),
        Block::Quote(lines) => lines.last().map(String::len),
        _ => None,
    }
}

/// Fade budget of each block: the trailing prose blocks that lie inside the
/// fading source bytes fade, earlier ones and non-prose blocks stay opaque.
fn block_fades(blocks: &[Block], fade: StreamFade) -> Vec<StreamFade> {
    let mut fades = vec![StreamFade::default(); blocks.len()];
    if !fade.active() {
        return fades;
    }
    let mut remaining = fade.bytes;
    for (index, block) in blocks.iter().enumerate().rev() {
        if remaining == 0 {
            break;
        }
        match block_prose_len(block) {
            Some(len) => {
                fades[index] = fade.with_bytes(remaining.min(len));
                // Markers and the blank line between blocks.
                remaining = remaining.saturating_sub(len + 2);
            }
            // Code, tables, and diagrams keep full contrast and end the
            // fading run: anything above them was released earlier.
            None => break,
        }
    }
    fades
}

/// Retain the native layout alongside the interactive leaf, so geometry tests
/// exercise exactly the same shaping, highlighting and links as the renderer.
fn styled_line_layout(
    source: &str,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    window: &gpui::Window,
    cx: &gpui::App,
    avatar: bool,
) -> (gpui::AnyElement, gpui::TextLayout) {
    styled_inline_layout(inline_spans(source), selection, key, window, cx, avatar)
}

fn styled_inline_layout(
    mut inline: Inline,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    window: &gpui::Window,
    cx: &gpui::App,
    avatar: bool,
) -> (gpui::AnyElement, gpui::TextLayout) {
    let transcript_links = TRANSCRIPT_LINKS.with_borrow(Clone::clone);
    let mentions = match &transcript_links {
        Some((handler, true)) => {
            let mentions = inline_file_mentions(&inline, &handler.base);
            probe_queued_file_mentions(cx);
            mentions
        }
        _ => Vec::new(),
    };
    let images = prompt_images::current();
    let markers = images.as_ref().map_or_else(Vec::new, |images| {
        crate::image_chip::image_marker_ranges(&inline.plain)
            .into_iter()
            .filter_map(|range| {
                if inline.links
                    .iter()
                    .any(|(link, _)| link.start < range.end && range.start < link.end)
                {
                    return None;
                }
                Some((range.clone(), images.attachments.get(&inline.plain[range])?))
            })
            .collect()
    });
    let mut highlights = std::mem::take(&mut inline.highlights);
    if !mentions.is_empty() {
        let style = HighlightStyle {
            color: Some(to_hsla(Theme::global().LINK)),
            ..Default::default()
        };
        highlights.extend(mentions.iter().map(|(range, _)| (range.clone(), style)));
    }
    highlights.extend(fade_tail_highlights(&inline.plain, LEAF_FADE.take()));
    highlights.extend(search_highlights(&inline.plain));
    if let Some(highlight) = selection.read(cx).highlight(&key, inline.plain.len()) {
        highlights.push(highlight);
    }
    let prefix = if avatar { AVATAR_PREFIX } else { "" };
    let (display, offsets, chips) = project_prompt_images(
        &inline.plain,
        prefix,
        &markers,
        gpui::font(Theme::global().FONT_UI),
        px(13.5),
        window,
    );
    let shift = |range| offsets.layout_range(range);
    let mut highlights: Vec<_> = highlights
        .into_iter()
        .map(|(range, style)| (shift(range), style))
        .collect();
    highlights.extend(
        chips
            .iter()
            .map(|(range, _, _)| (range.clone(), crate::image_chip::hidden_marker())),
    );
    #[cfg(test)]
    if images.is_some() {
        PAINTED_PROMPT_TEXT.with(|text| text.borrow_mut().push(display.clone()));
    }
    let display: SharedString = display.into();
    // Resolve the base style during layout, inside the surrounding element.
    // Capturing window.text_style() here bypasses the dimmed reasoning color
    // (and heading weight), since the parent has not been laid out yet.
    // Give the spacer its own monospace run. Four proportional spaces can be
    // narrower than the icon when the user changes the assistant font.
    let prefix_highlight = avatar.then(|| (0..prefix.len(), HighlightStyle::default()));
    let prefix_font = avatar.then(|| (0..prefix.len(), Theme::global().mono_family()));
    let text = StyledText::new(display)
        .with_highlights(
            prefix_highlight
                .into_iter()
                .chain(flatten_highlights(&highlights)),
        )
        .with_font_family_overrides(
            prefix_font.into_iter().chain(
                inline
                    .code_ranges
                    .iter()
                    .cloned()
                    .map(|range| (shift(range), Theme::global().mono_family())),
            ),
        );
    let layout = text.layout().clone();
    let hover_links = mentions.iter().map(|(range, _)| shift(range.clone())).collect();
    let child = if inline.links.is_empty() && mentions.is_empty() {
        text.into_any_element()
    } else {
        let id: SharedString = format!("md-link-{:x}", hash(&inline.plain)).into();
        let links = inline
            .links
            .drain(..)
            .map(|(range, url)| (range, InlineDestination::Markdown(url)))
            .chain(
                mentions
                    .into_iter()
                    .map(|(range, path)| (range, InlineDestination::Mention(path))),
            );
        let handler = FILE_OPTIONS
            .with(|slot| slot.borrow().as_ref().map(|file| file.on_link.clone()))
            .or_else(|| transcript_links.map(|(handler, _)| handler));
        let (source_ranges, destinations): (Vec<_>, Vec<_>) = links.unzip();
        #[cfg(any(test, debug_assertions))]
        let selector = handler
            .as_ref()
            .and_then(|_| destinations.first())
            .map(|destination| {
                let label = match destination {
                    InlineDestination::Markdown(url) => url,
                    InlineDestination::Mention(_) => &inline.plain[source_ranges[0].clone()],
                };
                format!("markdown-link:{label}")
            });
        let ranges = source_ranges.into_iter().map(shift).collect();
        let interactive =
            InteractiveText::new(id, text).on_click(ranges, move |index, window, cx| {
                match (&handler, &destinations[index]) {
                    (Some(handler), InlineDestination::Markdown(url)) => {
                        handler.open_markdown(url, window, cx);
                    }
                    (Some(handler), InlineDestination::Mention(path)) => handler.open(
                        LinkTarget::Local {
                            path: path.clone(),
                            fragment: None,
                        },
                        window,
                        cx,
                    ),
                    (None, InlineDestination::Markdown(url)) => {
                        if let Some(url) = openable_url(url) {
                            cx.open_url(url);
                        }
                    }
                    (None, InlineDestination::Mention(_)) => {}
                }
            });
        #[cfg(any(test, debug_assertions))]
        let interactive = if let Some(selector) = selector {
            div()
                .debug_selector(move || selector)
                .child(interactive)
                .into_any_element()
        } else {
            interactive.into_any_element()
        };
        interactive.into_any_element()
    };
    let child = crate::markdown_inline_code::wrap(
        child,
        layout.clone(),
        inline.code_ranges.iter().cloned().map(shift).collect(),
        hover_links,
        key.clone(),
    );
    let image_overlay = images.as_ref().filter(|_| !chips.is_empty()).map(|images| {
        let keys = markers.iter().map(|(range, _)| inline.plain[range.clone()].to_owned()).collect();
        crate::image_chip::PositionedChips::new(layout.clone(), chips, key.clone(), keys, images.on_click.clone())
    });
    #[cfg(test)]
    let capture = PAINTED_FONT_STYLES.with(|slot| {
        slot.borrow()
            .clone()
            .map(|styles| (inline.plain.clone(), styles))
    });
    let selectable = text_selection::selectable_with_offsets(
        selection.clone(),
        key,
        inline.plain,
        layout.clone(),
        child,
        offsets,
        cx,
    );
    // The selectable leaf already establishes a relative containing block.
    // Only an avatar needs another one for its absolutely positioned overlay.
    // Ordinary paragraphs, list items and table cells avoid a redundant div's
    // style refinement, layout node, prepaint and paint on every frame.
    let element = if avatar || image_overlay.is_some() {
        div()
            .relative()
            .child(selectable)
            .children(image_overlay)
            .when(avatar, |el| el.child(div().absolute().top_0().left_0().child(assistant_avatar())))
            .into_any_element()
    } else {
        selectable
    };
    #[cfg(test)]
    let element = if let Some((text, styles)) = capture {
        div()
            .relative()
            .child(element)
            .child(
                gpui::canvas(
                    |_, _, _| {},
                    move |_, (), window, _| {
                        styles.borrow_mut().insert(text, window.text_style().font_style);
                    },
                )
                .absolute(),
            )
            .into_any_element()
    } else {
        element
    };
    (element, layout)
}

pub(crate) const PINNED_PROMPT_FONT_SIZE: f32 = 13.5;
const PINNED_PROMPT_LINES: usize = 3;

pub(crate) struct PinnedPromptPreview(String);

impl PinnedPromptPreview {
    pub(crate) fn new(source: &str) -> Self {
        // A fourth hard line is already enough to prove overflow. Still use
        // GPUI to find earlier soft wrapping, including the widths of chips.
        let source = source.trim_end();
        let end = source
            .match_indices('\n')
            .nth(PINNED_PROMPT_LINES)
            .map_or(source.len(), |(i, _)| i);
        Self(inline_spans(&source[..end]).plain)
    }

    pub(crate) fn visible_prefix(&self, width: gpui::Pixels, window: &gpui::Window) -> &str {
        let images = prompt_images::current();
        let markers = images.as_ref().map_or_else(Vec::new, |images| {
            crate::image_chip::image_marker_ranges(&self.0)
                .into_iter()
                .filter_map(|range| {
                    let image = images.attachments.get(&self.0[range.clone()])?;
                    Some((range, image))
                })
                .collect()
        });
        let font = gpui::font(Theme::global().FONT_UI);
        let size = px(PINNED_PROMPT_FONT_SIZE);
        let (display, offsets, chips) =
            project_prompt_images(&self.0, "", &markers, font.clone(), size, window);
        let runs = [gpui::TextRun {
            len: display.len(),
            font,
            ..Default::default()
        }];
        let Ok(layouts) =
            window
                .text_system()
                .shape_text(display.into(), size, &runs, Some(width), None)
        else {
            return "";
        };
        let mut remaining = PINNED_PROMPT_LINES;
        let mut start = 0;
        for line in layouts {
            if remaining == 0 {
                return self.prefix_at(start - 1, &offsets, &chips);
            }
            if let Some(boundary) = line.wrap_boundaries().get(remaining - 1) {
                let glyph = &line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix];
                return self.prefix_at(start + glyph.index, &offsets, &chips);
            }
            remaining -= line.wrap_boundaries().len() + 1;
            start += line.len() + 1;
        }
        &self.0
    }

    fn prefix_at(
        &self,
        mut end: usize,
        offsets: &text_selection::LayoutOffsets,
        chips: &[ImageChip],
    ) -> &str {
        if let Some((range, _, _)) = chips
            .iter()
            .find(|(range, _, _)| range.start < end && end < range.end)
        {
            end = range.start;
        }
        self.0[..offsets.source_index(end, self.0.len())].trim_end()
    }

    pub(crate) fn overflows(&self, prefix: &str) -> bool {
        prefix.len() < self.0.len()
    }

    pub(crate) fn render(
        prefix: &str,
        selection: &gpui::Entity<TextSelection>,
        key: SharedString,
        window: &gpui::Window,
        cx: &gpui::App,
        background: gpui::Rgba,
    ) -> gpui::AnyElement {
        let (child, layout) = styled_inline_layout(
            Inline {
                plain: prefix.to_owned(),
                code_ranges: Vec::new(),
                highlights: Vec::new(),
                links: Vec::new(),
            },
            selection,
            key,
            window,
            cx,
            false,
        );
        div()
            .font_family(Theme::global().FONT_UI)
            .font_weight(FontWeight::NORMAL)
            .text_size(px(PINNED_PROMPT_FONT_SIZE))
            .line_height(relative(1.55))
            .child(crate::prompt_background::wrap(child, layout, background))
            .into_any_element()
    }
}


/// Logical reading order for transcript selection without laying out offscreen rows.
/// Keys match selectable render leaves, including reasoning namespaces and padded
/// table cells. List markers and rules are decorations, not selectable text.
/// Diff/patch, rendered math, HTML previews, and Mermaid use synthetic block-key
/// entries containing their readable source: these rich/media views do not expose
/// a corresponding selectable leaf (HTML in reasoning does render as code).
pub(crate) fn selection_segments(
    source: &str,
    row: usize,
    reasoning: bool,
    prompt: bool,
) -> Vec<(SharedString, SharedString)> {
    selection_segments_with_prefix(source, &row.to_string(), reasoning, prompt)
}

pub(crate) fn selection_segments_with_prefix(
    source: &str,
    key_prefix: &str,
    reasoning: bool,
    prompt: bool,
) -> Vec<(SharedString, SharedString)> {
    let mut segments = Vec::new();
    collect_selection_segments(source, key_prefix, reasoning, prompt, &mut segments);
    segments
}

fn collect_selection_segments(
    source: &str,
    key_prefix: &str,
    _reasoning: bool,
    prompt: bool,
    segments: &mut Vec<(SharedString, SharedString)>,
) {
    let document = cached_parse(source, prompt);
    for (block_index, block) in document.blocks.iter().enumerate() {
        let key = format!("{key_prefix}-{block_index}");
        match block {
            Block::Paragraph(text) => {
                segments.push((key.into(), inline_spans(text).plain.into()));
            }
            Block::Heading(_, text) | Block::Bullet { text, .. } | Block::Numbered { text, .. } => {
                segments.push((key.into(), inline_spans(text).plain.into()));
            }
            Block::Reasoning(text) => {
                collect_selection_segments(text, &key, true, prompt, segments);
            }
            // Quotes currently render individual nonempty inline lines, not a
            // recursive markdown document. Preserve that exact leaf structure.
            Block::Quote(lines) => {
                for (line_index, line) in lines
                    .iter()
                    .filter(|line| !line.trim().is_empty())
                    .enumerate()
                {
                    segments.push((
                        format!("{key}-{line_index}").into(),
                        inline_spans(line).plain.into(),
                    ));
                }
            }
            Block::Table { header, rows } => {
                let columns = header
                    .len()
                    .max(rows.iter().map(Vec::len).max().unwrap_or(0));
                for (row_index, row) in std::iter::once(header).chain(rows.iter()).enumerate() {
                    for column in 0..columns {
                        let text = row.get(column).map(String::as_str).unwrap_or("");
                        segments.push((
                            table_cell_key(&key, row_index, column),
                            inline_spans(text).plain.into(),
                        ));
                    }
                }
            }
            // Highlighting preserves code's text. Avoid syntax highlighting and
            // media rendering here, so offscreen extraction stays inexpensive.
            Block::Code { body, .. }
            | Block::HtmlPreview(body)
            | Block::Mermaid(body)
            | Block::Math(body) => {
                segments.push((key.into(), body.as_str().into()));
            }
            Block::Rule => {}
        }
    }
}

/// Header is row zero, followed by body rows. Content never participates in IDs.
fn table_cell_key(key: &str, row: usize, column: usize) -> SharedString {
    format!("{key}-{row}-{column}").into()
}

/// Render markdown into a column of GPUI elements.
pub fn render(
    source: &str,
    row: usize,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
) -> impl IntoElement {
    render_with_style(source, row, selection, window, cx, false, None)
}

/// Render user prose with preserved newlines and a rounded, line-fitting fill.
#[allow(dead_code, reason = "kept for callers that render prompts by hand")]
pub(crate) fn render_prompt(
    source: &str,
    row: usize,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    on_preview: MediaPreviewHandler,
    background: gpui::Rgba,
) -> gpui::AnyElement {
    render_prompt_with_prefix(
        source,
        row,
        &row.to_string(),
        selection,
        window,
        cx,
        on_preview,
        background,
    )
}

/// Render a duplicate prompt in its own selection namespace. Pinned overlays
/// must not reuse the transcript's logical row keys or replace its text leaves.
#[allow(clippy::too_many_arguments, reason = "render signature carries the row's data")]
pub(crate) fn render_prompt_with_prefix(
    source: &str,
    row: usize,
    key_prefix: &str,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    on_preview: MediaPreviewHandler,
    background: gpui::Rgba,
) -> gpui::AnyElement {
    render_document_with_prompt_background(
        source,
        row,
        key_prefix,
        selection,
        window,
        cx,
        false,
        Some(on_preview),
        false,
        Some(background),
    )
}

/// Render transcript media with a panel-owned lightbox callback.
pub(crate) fn render_interactive(
    source: &str,
    row: usize,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: MediaPreviewHandler,
) -> impl IntoElement {
    render_interactive_with_avatar(
        source, row, selection, window, cx, reasoning, on_preview, false,
    )
}

pub(crate) fn render_inline_answer(
    source: &str,
    row: usize,
    key_prefix: &str,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    on_preview: MediaPreviewHandler,
) -> impl IntoElement {
    render_document(
        source,
        row,
        key_prefix,
        selection,
        window,
        cx,
        false,
        Some(on_preview),
        false,
    )
}

#[allow(clippy::too_many_arguments, reason = "render signature carries the row's data")]
pub(crate) fn render_interactive_with_avatar(
    source: &str,
    row: usize,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: MediaPreviewHandler,
    avatar: bool,
) -> impl IntoElement {
    render_document(
        source,
        row,
        &row.to_string(),
        selection,
        window,
        cx,
        reasoning,
        Some(on_preview),
        avatar,
    )
}

fn render_with_style(
    source: &str,
    row: usize,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: Option<MediaPreviewHandler>,
) -> impl IntoElement {
    render_document(
        source,
        row,
        &row.to_string(),
        selection,
        window,
        cx,
        reasoning,
        on_preview,
        false,
    )
}

#[allow(clippy::too_many_arguments, reason = "render signature carries the row's data")]
fn render_document(
    source: &str,
    row: usize,
    key_prefix: &str,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: Option<MediaPreviewHandler>,
    avatar: bool,
) -> gpui::AnyElement {
    render_document_with_prompt_background(
        source, row, key_prefix, selection, window, cx, reasoning, on_preview, avatar, None,
    )
}

#[allow(clippy::too_many_arguments, reason = "render signature carries the row's data")]
fn render_document_with_prompt_background(
    source: &str,
    row: usize,
    key_prefix: &str,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: Option<MediaPreviewHandler>,
    avatar: bool,
    prompt_background: Option<gpui::Rgba>,
) -> gpui::AnyElement {
    let document = cached_parse(source, prompt_background.is_some());
    render_blocks(
        &document.blocks,
        row,
        key_prefix,
        selection,
        window,
        cx,
        reasoning,
        on_preview,
        avatar,
        prompt_background,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "render signature carries the row's data"
)]
fn render_blocks(
    blocks: &[Block],
    row: usize,
    key_prefix: &str,
    selection: &gpui::Entity<TextSelection>,
    window: &gpui::Window,
    cx: &gpui::App,
    reasoning: bool,
    on_preview: Option<MediaPreviewHandler>,
    avatar: bool,
    prompt_background: Option<gpui::Rgba>,
) -> gpui::AnyElement {
    let mut children: Vec<gpui::AnyElement> = Vec::with_capacity(blocks.len());
    let mut previous_was_list = false;
    let fade = STREAM_FADE.take();
    let fades = if fade.active() {
        block_fades(blocks, fade)
    } else {
        Vec::new()
    };

    for (block_index, block) in blocks.iter().enumerate() {
        // Only prose leaves fade. Table cells and code keep full contrast.
        LEAF_FADE.set(fades.get(block_index).copied().unwrap_or_default());
        let text_key = || -> SharedString { format!("{key_prefix}-{block_index}").into() };
        let is_list = matches!(block, Block::Bullet { .. } | Block::Numbered { .. });
        let tight = is_list && previous_was_list;
        previous_was_list = is_list;

        let first_line_avatar = avatar && block_index == 0;
        let fitted_background = matches!(
            &block,
            Block::Heading(..) | Block::Paragraph(..) | Block::Reasoning(..)
        );
        let inline_text = |text: &str| {
            let (child, layout) =
                styled_line_layout(text, selection, text_key(), window, cx, first_line_avatar);
            match prompt_background {
                Some(color) => crate::prompt_background::wrap(child, layout, color),
                None => child,
            }
        };
        // Structured non-inline blocks get a separate mark, never an overlay
        // on code, table cells, diagrams, or media.
        let inline_avatar = matches!(
            &block,
            Block::Heading(..)
                | Block::Paragraph(..)
                | Block::Reasoning(..)
                | Block::Bullet { .. }
                | Block::Numbered { .. }
                | Block::Quote(..)
        );
        if first_line_avatar && !inline_avatar {
            children.push(
                div()
                    .h(px(22.0))
                    .child(assistant_avatar())
                    .into_any_element(),
            );
        }
        let element = match block {
            Block::Heading(level, text) => {
                let (size, weight) = match level {
                    _ if reasoning => (px(12.0), FontWeight::SEMIBOLD),
                    1 => (px(19.0), FontWeight::BOLD),
                    2 => (px(16.5), FontWeight::BOLD),
                    3 => (px(14.5), FontWeight::SEMIBOLD),
                    _ => (px(13.5), FontWeight::SEMIBOLD),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(reasoning, |el| el.mt_1().mb_1())
                    .when(!reasoning, |el| el.mt_2())
                    .child(
                        div()
                            .text_size(size)
                            .font_weight(weight)
                            .text_color(if reasoning {
                                Theme::global().REASONING
                            } else {
                                Theme::global().HEADING
                            })
                            .line_height(relative(1.35))
                            .child(inline_text(text)),
                    )
                    .when(!reasoning && *level <= 2, |el| {
                        el.child(div().h(px(1.0)).w_full().bg(Theme::global().PANEL_BORDER))
                    })
                    .into_any_element()
            }
            Block::Reasoning(text) => div()
                .debug_selector(|| "restored-reasoning".into())
                .text_size(px(12.0))
                .italic()
                .text_color(Theme::global().REASONING)
                .line_height(relative(1.55))
                .child(render_document_with_prompt_background(
                    text,
                    row,
                    &format!("{key_prefix}-{block_index}"),
                    selection,
                    window,
                    cx,
                    true,
                    on_preview.clone(),
                    first_line_avatar,
                    prompt_background,
                ))
                .into_any_element(),
            Block::Paragraph(text) => {
                if FILE_OPTIONS.with(|slot| slot.borrow().is_some()) && text.contains("![") {
                    div()
                        .line_height(relative(1.55))
                        .child(styled_line_with_avatar(
                            text,
                            selection,
                            text_key(),
                            window,
                            cx,
                            first_line_avatar,
                        ))
                        .into_any_element()
                } else {
                    div()
                        .line_height(relative(1.55))
                        .child(inline_text(text))
                        .into_any_element()
                }
            }
            Block::Bullet { depth, text, task } => {
                let marker = match task {
                    Some(true) => "✓".to_string(),
                    Some(false) => "○".to_string(),
                    None => match depth {
                        0 => "•".into(),
                        1 => "◦".into(),
                        _ => "▪".into(),
                    },
                };
                let marker_color = match task {
                    _ if reasoning => Theme::global().REASONING,
                    Some(true) => Theme::global().OK,
                    Some(false) => Theme::global().TEXT_DIM,
                    None => Theme::global().ACCENT_MUTED,
                };
                list_row(
                    *depth,
                    marker,
                    marker_color,
                    text,
                    selection,
                    text_key(),
                    window,
                    cx,
                    tight,
                    *task,
                    first_line_avatar,
                )
            }
            Block::Numbered {
                depth,
                number,
                text,
            } => list_row(
                *depth,
                format!("{number}."),
                if reasoning {
                    Theme::global().REASONING
                } else {
                    Theme::global().ACCENT_MUTED
                },
                text,
                selection,
                text_key(),
                window,
                cx,
                tight,
                None,
                first_line_avatar,
            ),
            Block::Quote(lines) => div()
                .debug_selector(|| "md-quote".into())
                .flex()
                .flex_col()
                .my_1()
                .border_l_2()
                .border_color(Theme::global().PANEL_BORDER)
                .bg(Theme::global().QUOTE_BG)
                .rounded_r_md()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_2p5()
                        .py_1()
                        .flex_1()
                        .min_w_0()
                        .text_color(Theme::global().TEXT_DIM)
                        .italic()
                        .line_height(relative(1.5))
                        .children({
                            let fade = LEAF_FADE.take();
                            let lines: Vec<&String> = lines
                                .iter()
                                .filter(|line| !line.trim().is_empty())
                                .collect();
                            let last_line = lines.len().saturating_sub(1);
                            lines
                                .into_iter()
                                .enumerate()
                                .map(move |(line_index, line)| {
                                    LEAF_FADE.set(if line_index == last_line {
                                        fade
                                    } else {
                                        StreamFade::default()
                                    });
                                    div().child(styled_line_with_avatar(
                                        line,
                                        selection,
                                        format!("{key_prefix}-{block_index}-{line_index}").into(),
                                        window,
                                        cx,
                                        first_line_avatar && line_index == 0,
                                    ))
                                })
                        }),
                )
                .into_any_element(),
            Block::Code { lang, body }
                if lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch") =>
            {
                crate::diff_block::DiffBlock::new(body, text_key()).into_any_element()
            }
            Block::Code { lang, body } => {
                code_block_with_selection(lang, body, window, Some((selection, text_key(), cx)))
            }
            Block::HtmlPreview(body)
                if !reasoning && FILE_OPTIONS.with(|slot| slot.borrow().is_none()) =>
            {
                crate::html_preview::HtmlPreview::new(body.clone(), row, block_index)
                    .into_any_element()
            }
            Block::HtmlPreview(body) => code_block_with_selection(
                "html-preview",
                body,
                window,
                Some((selection, text_key(), cx)),
            ),
            Block::Mermaid(body) => mermaid_diagram(body, text_key(), on_preview.clone()),
            Block::Table { header, rows } => table(header, rows, selection, text_key(), window, cx),
            Block::Math(source) => math_block(source, selection, text_key(), window, cx),
            Block::Rule => div()
                .h(px(1.0))
                .my_2()
                .w_full()
                .bg(Theme::global().PANEL_BORDER)
                .into_any_element(),
        };
        // Code, tables, and diagrams do not consume the fade. Never leak it.
        LEAF_FADE.set(StreamFade::default());
        // Blank-line block breaks need visibly more room than a wrapped line,
        // while consecutive list items stay tight. A uniform flex gap made
        // paragraphs read as one run of text.
        if block_index > 0 {
            let space = if tight {
                px(2.0)
            } else if reasoning {
                px(7.0)
            } else {
                px(11.0)
            };
            children.push(div().flex_none().h(space).into_any_element());
        }
        // Structured blocks retain their native rectangular geometry. Prose
        // instead gets a contour from the actual shaped visual line widths.
        children.push(match prompt_background {
            Some(color) if !fitted_background => {
                crate::prompt_background::wrap_block(element, color)
            }
            _ => element,
        });
    }

    let file = (key_prefix == "file")
        .then(|| FILE_OPTIONS.with(|slot| slot.borrow().clone()))
        .flatten();
    let root = div().flex().flex_col();
    if let Some(file) = file {
        root.id("file-markdown-scroll")
            .debug_selector(|| "markdown-file-preview".into())
            .w_full()
            .flex_1()
            .min_h_0()
            .p_4()
            .overflow_y_scroll()
            .track_scroll(&file.scroll)
            .children(
                children
                    .into_iter()
                    .map(|block| div().flex_none().min_w_0().child(block)),
            )
            .into_any_element()
    } else {
        root.children(children).into_any_element()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "render signature carries the row's data"
)]
fn list_row(
    depth: usize,
    #[allow(clippy::too_many_arguments)] marker: String,
    marker_color: gpui::Rgba,
    text: &str,
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    window: &gpui::Window,
    cx: &gpui::App,
    _tight: bool,
    task: Option<bool>,
    avatar: bool,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .pl(px(depth as f32 * 14.0))
        .child(
            div()
                .flex_none()
                .min_w(px(14.0))
                .text_color(marker_color)
                .text_size(px(12.5))
                .line_height(relative(1.55))
                .child(marker),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_height(relative(1.55))
                .when(task == Some(true), |el| {
                    el.text_color(Theme::global().TEXT_DIM)
                })
                .child(styled_line_with_avatar(
                    text, selection, key, window, cx, avatar,
                )),
        )
        .into_any_element()
}

fn table(
    header: &[String],
    rows: &[Vec<String>],
    selection: &gpui::Entity<TextSelection>,
    key: SharedString,
    window: &gpui::Window,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let cell = |text: &str, row: usize, column: usize, window: &gpui::Window| {
        div()
            .flex_1()
            .min_w_0()
            .px_2p5()
            .py_1p5()
            .line_height(relative(1.45))
            .child(styled_line(
                text,
                selection,
                table_cell_key(&key, row, column),
                window,
                cx,
            ))
    };
    div()
        .flex()
        .flex_col()
        .my_1()
        .w_full()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(Theme::global().PANEL_BORDER)
        .text_size(px(12.5))
        .child(
            div()
                .flex()
                .flex_row()
                .bg(Theme::global().CODE_HEADER_BG)
                .border_b_1()
                .border_color(Theme::global().PANEL_BORDER)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::global().HEADING)
                .children((0..columns).map(|index| {
                    cell(
                        header.get(index).map(String::as_str).unwrap_or(""),
                        0,
                        index,
                        window,
                    )
                })),
        )
        .children(rows.iter().enumerate().map(|(row_index, row)| {
            div()
                .flex()
                .flex_row()
                .when(row_index % 2 == 1, |el| el.bg(Theme::global().TABLE_STRIPE))
                .text_color(Theme::global().TEXT)
                .children((0..columns).map(|index| {
                    cell(
                        row.get(index).map(String::as_str).unwrap_or(""),
                        row_index + 1,
                        index,
                        window,
                    )
                }))
        }))
        .into_any_element()
}
