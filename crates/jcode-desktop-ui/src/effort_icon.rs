//! Drawn reasoning-effort marks.
//!
//! The composer bar, the `/effort` picker and the worker rows draw the level
//! as a shape, never as a text glyph: a circle the size of the other bar icons
//! with a sector filled to the rung, inked on the heat scale (`effort::tint`).
//! Max is a solid disc with a halo, the swarm modes are a connected cluster.
//! Painting goes through a canvas, so no font decides how a rung looks.

use gpui::{AnyElement, Bounds, PathBuilder, Pixels, Point, Rgba, Window, canvas, div, point};
use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, px};
use gpui::prelude::FluentBuilder;

use crate::theme::Theme;

/// What a level draws as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    /// Reasoning off: an empty ring crossed by a slash.
    Off,
    /// A ring with a sector filled from 12 o'clock, `fraction` of a turn.
    Sector(f32),
    /// The top rung: a solid disc inside a glowing halo.
    Max,
    /// Orchestration modes: a cluster of connected nodes.
    Swarm { deep: bool },
}

/// The shape for a level, or `None` for a value that names no known rung, so
/// an unknown level keeps its word rather than borrowing a shape.
pub(crate) fn shape(effort: &str) -> Option<Shape> {
    Some(match effort.trim().to_ascii_lowercase().as_str() {
        "none" => Shape::Off,
        "minimal" => Shape::Sector(0.125),
        "low" => Shape::Sector(0.25),
        "medium" => Shape::Sector(0.5),
        "high" => Shape::Sector(0.75),
        "xhigh" => Shape::Sector(0.875),
        "max" => Shape::Max,
        "swarm" => Shape::Swarm { deep: false },
        "swarm-deep" => Shape::Swarm { deep: true },
        _ => return None,
    })
}

/// Ink for a level: the heat scale, with the swarm modes in the accent so they
/// read as their own thing rather than the faint bottom rung.
pub(crate) fn ink(effort: &str, theme: &Theme) -> Rgba {
    match shape(effort) {
        Some(Shape::Swarm { .. }) => theme.ACCENT,
        _ => crate::effort::tint(effort, theme),
    }
}

/// The drawn mark for `effort` at `size` points, or `None` for an unknown
/// level. The element carries `effort-icon-<level>` for tests.
pub(crate) fn icon(effort: &str, size: f32) -> Option<AnyElement> {
    draw(effort, size, None)
}

/// Picker rows invert their paper on selection. The mark keeps its own opaque
/// paper and a solid outline, so neither hover nor selection washes it out.
pub(crate) fn picker_icon(effort: &str, size: f32) -> Option<AnyElement> {
    draw(effort, size, Some(picker_palette(effort, Theme::global())))
}

fn draw(effort: &str, size: f32, palette: Option<(Rgba, Rgba)>) -> Option<AnyElement> {
    let shape = shape(effort)?;
    let key = effort.trim().to_ascii_lowercase();
    let ink = palette.map_or_else(|| ink(effort, Theme::global()), |(ink, _)| ink);
    Some(
        div()
            .debug_selector(move || format!("effort-icon-{key}"))
            .flex_none()
            .size(px(size + if palette.is_some() { 4. } else { 0. }))
            .when_some(palette, |el, (_, bg)| el.rounded_full().bg(bg).p(px(2.)))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| paint(shape, bounds, ink, palette.is_some(), window),
                )
                .size_full(),
            )
            .into_any_element(),
    )
}

fn luminance(color: Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn picker_palette(effort: &str, theme: &Theme) -> (Rgba, Rgba) {
    let bg = Rgba { a: 1., ..theme.HEADER_BG };
    let color = Rgba { a: 1., ..ink(effort, theme) };
    let paper = luminance(bg);
    let edge = if contrast(paper, 0.) >= contrast(paper, 1.) { 0. } else { 1. };
    let fg = (0_u8..8)
        .map(|step| {
            let toward_edge = f32::from(step) / 8.;
            let mix = |c| c + (edge - c) * toward_edge;
            Rgba { r: mix(color.r), g: mix(color.g), b: mix(color.b), a: 1. }
        })
        .find(|fg| contrast(luminance(*fg), paper) >= 3.5)
        .unwrap_or(Rgba { r: edge, g: edge, b: edge, a: 1. });
    (fg, bg)
}

fn faded(color: Rgba, alpha: f32, solid: bool) -> Rgba {
    Rgba {
        a: color.a * if solid { 1. } else { alpha },
        ..color
    }
}

fn paint(shape: Shape, bounds: Bounds<Pixels>, ink: Rgba, solid: bool, window: &mut Window) {
    let center = bounds.center();
    let size = f32::from(bounds.size.width.min(bounds.size.height));
    let outer = size / 2.;
    // Ring weight matches the context gauge (2.6 at 13pt) a touch lighter, so
    // the sector inside stays legible.
    let weight = (size * 0.15).max(1.4);
    match shape {
        Shape::Off => {
            fill(
                window,
                annulus(center, outer, outer - weight),
                faded(ink, 0.8, solid),
            );
            let d = outer * 0.55;
            let mut slash = PathBuilder::stroke(px(weight * 0.8));
            slash.move_to(center + point(px(-d), px(d)));
            slash.line_to(center + point(px(d), px(-d)));
            fill(window, slash.build().ok(), ink);
        }
        Shape::Sector(fraction) => {
            fill(
                window,
                annulus(center, outer, outer - weight),
                faded(ink, 0.45, solid),
            );
            fill(window, sector(center, outer - weight - 1.0, fraction), ink);
        }
        Shape::Max => {
            fill(
                window,
                annulus(center, outer, outer - weight),
                faded(ink, 0.4, solid),
            );
            fill(window, sector(center, outer - weight - 0.6, 1.), ink);
        }
        Shape::Swarm { deep } => {
            let reach = outer * 0.6;
            let node = (size * 0.16).max(1.6);
            let nodes = [-90f32, 150., 30.].map(|degrees| {
                let angle = degrees.to_radians();
                center + point(px(reach * angle.cos()), px(reach * angle.sin()))
            });
            let mut links = PathBuilder::stroke(px((weight * 0.6).max(1.0)));
            links.move_to(nodes[0]);
            links.line_to(nodes[1]);
            links.line_to(nodes[2]);
            links.close();
            fill(window, links.build().ok(), faded(ink, 0.55, solid));
            for at in nodes {
                fill(window, sector(at, node, 1.), ink);
            }
            if deep {
                fill(window, sector(center, node * 0.8, 1.), ink);
            }
        }
    }
}

fn fill(window: &mut Window, path: Option<gpui::Path<Pixels>>, color: Rgba) {
    if let Some(path) = path {
        window.paint_path(path, color);
    }
}

fn at(center: Point<Pixels>, r: f32, turn: f32) -> Point<Pixels> {
    let angle = turn * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
    center + point(px(r * angle.cos()), px(r * angle.sin()))
}

/// A pie slice from 12 o'clock, clockwise; a full disc at `fraction >= 1`.
fn sector(center: Point<Pixels>, r: f32, fraction: f32) -> Option<gpui::Path<Pixels>> {
    let fraction = fraction.clamp(0., 1.);
    if fraction <= 0. || r <= 0. {
        return None;
    }
    let radii = point(px(r), px(r));
    let mut builder = PathBuilder::fill();
    if fraction >= 0.999 {
        builder.move_to(at(center, r, 0.));
        builder.arc_to(radii, px(0.), false, true, at(center, r, 0.5));
        builder.arc_to(radii, px(0.), false, true, at(center, r, 0.));
    } else {
        builder.move_to(center);
        builder.line_to(at(center, r, 0.));
        builder.arc_to(radii, px(0.), fraction > 0.5, true, at(center, r, fraction));
    }
    builder.close();
    builder.build().ok()
}

/// A full ring between two radii.
fn annulus(center: Point<Pixels>, outer: f32, inner: f32) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::fill();
    let o = point(px(outer), px(outer));
    builder.move_to(at(center, outer, 0.));
    builder.arc_to(o, px(0.), false, true, at(center, outer, 0.5));
    builder.arc_to(o, px(0.), false, true, at(center, outer, 0.));
    builder.close();
    let i = point(px(inner), px(inner));
    builder.move_to(at(center, inner, 0.));
    builder.arc_to(i, px(0.), false, false, at(center, inner, 0.5));
    builder.arc_to(i, px(0.), false, false, at(center, inner, 0.));
    builder.close();
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_rung_marks_have_at_least_three_to_one_contrast() {
        let mut minimum = 21.0_f32;
        for preset in crate::theme::ThemePreset::ALL {
            let theme = Theme::preview(preset);
            for level in ["none", "minimal", "low", "medium", "high", "xhigh", "max", "swarm", "swarm-deep"] {
                let (fg, bg) = picker_palette(level, theme);
                assert_eq!(bg.a, 1., "backplate must isolate the mark from either row color");
                assert_eq!(faded(fg, 0.4, true).a, 1.);
                let ratio = contrast(luminance(fg), luminance(bg));
                assert!(ratio >= 3., "{preset:?} {level}: {ratio:.2}:1");
                minimum = minimum.min(ratio);
            }
        }
        eprintln!("Minimum picker glyph contrast across all presets and levels: {minimum:.2}:1");
    }

    /// Every rung and mode the picker offers is a drawn shape, the fill grows
    /// with the level, and max and the swarm modes get their own marks.
    #[test]
    fn every_level_is_a_drawn_shape_that_grows_with_the_rung() {
        let rungs = ["minimal", "low", "medium", "high", "xhigh"];
        let fills: Vec<f32> = rungs
            .iter()
            .map(|level| match shape(level) {
                Some(Shape::Sector(fraction)) => fraction,
                other => panic!("{level} draws {other:?}"),
            })
            .collect();
        assert!(fills.windows(2).all(|pair| pair[0] < pair[1]), "{fills:?}");
        assert_eq!(shape("none"), Some(Shape::Off));
        assert_eq!(shape(" Max "), Some(Shape::Max));
        assert_eq!(shape("swarm"), Some(Shape::Swarm { deep: false }));
        assert_eq!(shape("swarm-deep"), Some(Shape::Swarm { deep: true }));
        assert_eq!(shape("future"), None);
    }

    #[test]
    fn swarm_modes_are_inked_apart_from_the_faint_bottom_rung() {
        let theme = Theme::global();
        assert_eq!(ink("swarm", theme), theme.ACCENT);
        assert_eq!(ink("high", theme), crate::effort::tint("high", theme));
    }
}
