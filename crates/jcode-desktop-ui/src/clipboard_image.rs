//! Clipboard image extraction, preferring the compositor's encoded bytes.

use std::{borrow::Cow, path::PathBuf};

pub(crate) enum ClipboardContent {
    Files(Vec<PathBuf>),
    Image(ClipboardImage),
    Empty,
}

fn classify_paste(
    files: Vec<PathBuf>,
    image: impl FnOnce() -> Result<Option<ClipboardImage>, String>,
) -> Result<ClipboardContent, String> {
    if !files.is_empty() {
        return Ok(ClipboardContent::Files(files));
    }
    Ok(image()?.map_or(ClipboardContent::Empty, ClipboardContent::Image))
}

pub(crate) fn read_paste(_cx: &gpui::App) -> Result<ClipboardContent, String> {
    #[cfg(test)]
    if let Some(reader) = _cx.try_global::<TestPasteReader>() {
        return (reader.0)();
    }
    let files = arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get().file_list())
        .unwrap_or_default();
    classify_paste(files, read)
}

#[cfg(test)]
pub(crate) struct TestPasteReader(pub fn() -> Result<ClipboardContent, String>);

#[cfg(test)]
impl gpui::Global for TestPasteReader {}

#[cfg(test)]
mod paste_tests {
    use super::*;

    #[test]
    fn no_files_allows_bitmap_fallback() {
        let result = classify_paste(Vec::new(), || Ok(None));
        assert!(matches!(result, Ok(ClipboardContent::Empty)));
    }

    #[test]
    fn finder_files_take_precedence_without_reading_their_preview_bitmap() {
        let files = vec![
            PathBuf::from("/tmp/notes.txt"),
            PathBuf::from("/tmp/image.png"),
        ];
        let result = classify_paste(files.clone(), || panic!("Finder preview was read"));
        assert!(matches!(result, Ok(ClipboardContent::Files(actual)) if actual == files));
    }
}

#[derive(Clone, Debug)]
pub struct ClipboardImage {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl ClipboardImage {
    pub fn label(&self) -> String {
        format!("{}×{}", self.width, self.height)
    }
}

const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Read an image without accidentally choosing the text/URI flavour commonly
/// published alongside copied images.
pub fn read() -> Result<Option<ClipboardImage>, String> {
    if let Some((media_type, bytes)) = read_wayland() {
        return decode_metadata(media_type, bytes).map(Some);
    }

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(error) => return Err(error.to_string()),
    };
    match clipboard.get_image() {
        Ok(image) => {
            let width = image.width as u32;
            let height = image.height as u32;
            let rgba: Cow<'_, [u8]> = image.bytes;
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(width, height, rgba.into_owned())
                    .ok_or_else(|| "clipboard returned malformed RGBA data".to_string())?,
            )
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
            Ok(Some(ClipboardImage {
                media_type: "image/png".into(),
                bytes: cursor.into_inner(),
                width,
                height,
            }))
        }
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn read_wayland() -> Option<(String, Vec<u8>)> {
    std::env::var_os("WAYLAND_DISPLAY")?;
    let listed = std::process::Command::new("wl-paste")
        .arg("--list-types")
        .output()
        .ok()?;
    if !listed.status.success() {
        return None;
    }
    let offered = String::from_utf8_lossy(&listed.stdout);
    let media_type = preferred_type(offered.lines())?;
    let read = std::process::Command::new("wl-paste")
        .args(["--no-newline", "--type", media_type])
        .output()
        .ok()?;
    (read.status.success() && !read.stdout.is_empty())
        .then(|| (media_type.to_string(), read.stdout))
}

fn decode_metadata(media_type: String, bytes: Vec<u8>) -> Result<ClipboardImage, String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|error| format!("could not identify clipboard image: {error}"))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| format!("could not read clipboard image: {error}"))?;
    Ok(ClipboardImage {
        media_type,
        bytes,
        width,
        height,
    })
}

/// Image bytes from another source, such as a file dropped on the window.
pub fn from_bytes(media_type: String, bytes: Vec<u8>) -> Result<ClipboardImage, String> {
    decode_metadata(media_type, bytes)
}

fn preferred_type<'a>(offered: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let offered: Vec<_> = offered.into_iter().collect();
    IMAGE_TYPES.into_iter().find(|wanted| {
        offered
            .iter()
            .any(|kind| kind.trim().eq_ignore_ascii_case(wanted))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_preferred_and_text_is_ignored() {
        assert_eq!(
            preferred_type(["image/jpeg", "image/png"]),
            Some("image/png")
        );
        assert_eq!(preferred_type(["text/plain", "text/uri-list"]), None);
    }
}
