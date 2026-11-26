//! Shared, bounded image preview geometry.
use base64::Engine;
use gpui::{Pixels, Size, px, size};

pub(crate) fn dimensions(encoded: &str) -> Option<(u32, u32)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Leave room for the toolbar and padding, without enlarging the image.
pub(crate) fn dialog_size(viewport: Size<Pixels>, dimensions: Option<(u32, u32)>) -> Size<Pixels> {
    let max_w = (f32::from(viewport.width) * 0.8).max(1.);
    let max_h = (f32::from(viewport.height) * 0.8).max(1.);
    let (width, height) = dimensions.unwrap_or((640, 480));
    let w = (width as f32).max(1.);
    let h = (height as f32).max(1.);
    let scale = ((max_w - 32.).max(1.) / w)
        .min((max_h - 100.).max(1.) / h)
        .min(1.);
    size(
        px((w * scale + 32.).max(300.).min(max_w)),
        px((h * scale + 100.).min(max_h)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_geometry_keeps_aspect_and_caps_both_orientations() {
        let viewport = size(px(800.), px(600.));
        for dimensions in [(1000, 600), (800, 1000)] {
            let modal = dialog_size(viewport, Some(dimensions));
            assert!(modal.width <= px(640.) && modal.height <= px(480.));
            let image_width = f32::from(modal.width) - 32.;
            let image_height = f32::from(modal.height) - 100.;
            assert!(
                (image_width / image_height - dimensions.0 as f32 / dimensions.1 as f32).abs()
                    < 0.01
            );
        }
    }
}
