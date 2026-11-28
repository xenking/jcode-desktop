//! Jcode Desktop's small component layer, in the spirit of Zed's `crates/ui`.
//!
//! Design rules (pill shapes, focus ring) and accessibility (`Role`, label,
//! keyboard shortcut) live here once, instead of being copied into every
//! screen's ad hoc `fn *_pill()` builder or one-off tooltip view.
//!
//! - [`Pill`]: every button-like control. Primary, secondary, danger, compact
//!   and ghost variants, an optional icon, a focus ring, `Role::Button` and an
//!   accessible label that defaults to the visible one.
//! - [`IconButton`]: a round icon-only toolbar control that floats over its row.
//! - [`Tooltip`]: the one hover view, for plain text, text with a key hint, or
//!   a title with rows.

mod icon_button;
mod pill;
mod tooltip;

pub use icon_button::IconButton;
pub use pill::{Pill, PillSemantics, PillVariant};
pub use tooltip::Tooltip;

use crate::theme::Theme;
use gpui::{BoxShadow, StyleRefinement, Styled, px};

/// The keyboard focus ring shared by every interactive component. Pass it to
/// `focus_visible`, so pointer clicks never show it.
pub fn focus_ring(style: StyleRefinement) -> StyleRefinement {
    style.shadow(vec![
        BoxShadow::new(px(0.), px(0.), crate::theme::to_hsla(Theme::global().TEXT))
            .spread_radius(px(2.))
            .inset(),
    ])
}

#[cfg(test)]
mod tests;
