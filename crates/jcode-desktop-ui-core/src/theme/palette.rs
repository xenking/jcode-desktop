//! One complete set of semantic colors and fonts, and the colors derived from
//! it. A palette is plain data: it knows nothing about which palette is active,
//! how switching fades, or where presets come from. That lives in
//! [`super::Theme`] and [`super::presets`], which both sit above this module.

use gpui::{Hsla, Rgba, SharedString, rgb};

use crate::config::AppearanceConfig;

#[derive(Clone)]
#[allow(non_snake_case)]
pub struct Palette {
    pub BG: Rgba,
    pub CANVAS_DOT: Rgba,
    pub PANEL_BG: Rgba,
    pub PANEL_BORDER: Rgba,
    pub PANEL_BORDER_FOCUS: Rgba,
    pub PANEL_BORDER_IDLE: Rgba,
    pub HEADER_BG: Rgba,
    pub TEXT: Rgba,
    pub TEXT_DIM: Rgba,
    pub TEXT_USER: Rgba,
    pub ACCENT: Rgba,
    pub ACCENT_DIM: Rgba,
    pub USER_ACCENT: Rgba,
    pub AI_ACCENT: Rgba,
    pub USER_BG: Rgba,
    /// Maximum age-rainbow wash. Neutral palettes can keep prompt paper untinted.
    pub PROMPT_TINT_STRENGTH: f32,
    pub TOOL_BG: Rgba,
    pub TOOL_TEXT: Rgba,
    pub REASONING: Rgba,
    pub REASONING_BG: Rgba,
    pub TEXT_FAINT: Rgba,
    pub TOOL_BORDER: Rgba,
    pub ERROR_BG: Rgba,
    pub CODE_BG: Rgba,
    pub CODE_TEXT: Rgba,
    pub INLINE_CODE_BG: Rgba,
    pub CODE_BORDER: Rgba,
    pub CODE_HEADER_BG: Rgba,
    pub CODE_GUTTER: Rgba,
    pub CODE_KEYWORD: Rgba,
    pub CODE_STRING: Rgba,
    pub CODE_COMMENT: Rgba,
    pub CODE_NUMBER: Rgba,
    pub CODE_TYPE: Rgba,
    pub CODE_PUNCT: Rgba,
    pub CODE_FUNCTION: Rgba,
    pub CODE_VARIABLE: Rgba,
    pub CODE_CONTROL: Rgba,
    pub CODE_CONSTANT: Rgba,
    pub CODE_TAG: Rgba,
    pub CODE_ATTRIBUTE: Rgba,
    pub ACCENT_MUTED: Rgba,
    pub QUOTE_BG: Rgba,
    pub TABLE_STRIPE: Rgba,
    pub INPUT_BG: Rgba,
    pub INPUT_BORDER: Rgba,
    pub CURSOR: Rgba,
    pub SELECTION: Rgba,
    pub ERROR: Rgba,
    pub OK: Rgba,
    pub WARN: Rgba,
    pub HEADING: Rgba,
    pub LINK: Rgba,
    pub MINIMAP_TRACK: Rgba,
    pub MINIMAP_TRACK_ACTIVE: Rgba,
    pub MINIMAP_VIEWPORT: Rgba,
    pub MINIMAP_PANEL: Rgba,
    pub MINIMAP_PANEL_BUSY: Rgba,
    pub MINIMAP_BG: Rgba,
    pub FONT_UI: &'static str,
    pub FONT_AI: &'static str,
    pub FONT_MONO: &'static str,
}

/// A palette color that views lay over a surface at partial strength, as a
/// hover fill, a selected-row wash or a status tint. Views name the ink and
/// the strength to [`Palette::wash`] and never derive from the color itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Text,
    TextFaint,
    Accent,
    AccentDim,
    Ok,
    Warn,
    Error,
    HeaderBg,
    PanelBg,
}

impl Palette {
    fn ink(&self, ink: Ink) -> Rgba {
        match ink {
            Ink::Text => self.TEXT,
            Ink::TextFaint => self.TEXT_FAINT,
            Ink::Accent => self.ACCENT,
            Ink::AccentDim => self.ACCENT_DIM,
            Ink::Ok => self.OK,
            Ink::Warn => self.WARN,
            Ink::Error => self.ERROR,
            Ink::HeaderBg => self.HEADER_BG,
            Ink::PanelBg => self.PANEL_BG,
        }
    }

    /// `ink` at `opacity` of its own alpha: a wash that tints whatever is
    /// painted beneath it.
    pub fn wash(&self, ink: Ink, opacity: f32) -> Rgba {
        self.ink(ink).opacity(opacity)
    }

    /// The monospace family as GPUI wants it for a text style.
    pub fn mono_family(&self) -> SharedString {
        self.FONT_MONO.into()
    }

    /// Popovers cover application content, not the desktop. Window vibrancy and
    /// translucent palettes must never let underlying transcript ink show through.
    pub fn popover_background(&self) -> Rgba {
        let mut paper = self.PANEL_BG;
        paper.a = 1.0;
        paper
    }

    /// A light wash of the CLI prompt-number rainbow in reverse hue order.
    /// Newest prompts start violet and older prompts move toward red, retaining
    /// the exponential fade back into the palette's card paper rather than gray.
    /// Backgrounds need much less color than the CLI's small foreground labels.
    pub fn prompt_background(&self, distance: usize) -> Rgba {
        const RAINBOW: [u32; 7] = [
            0xff5050, 0xffa050, 0xffe650, 0x50dc64, 0x50c8dc, 0x648cff, 0xb464ff,
        ];
        let tint = rgb(RAINBOW[RAINBOW.len() - 1 - distance.min(RAINBOW.len() - 1)]);
        let strength = self.PROMPT_TINT_STRENGTH * (-0.4 * distance as f32).exp();
        // Prompt cards cover transcript text, including with custom RGBA themes.
        // Blend a light tint into opaque paper, never into a translucent layer.
        let mut paper = self.USER_BG;
        paper.a = 1.0;
        paper.blend(tint.opacity(strength))
    }

    /// Workspace numbers and selection use the theme's neutral ink, not
    /// per-workspace colors. Numbers provide the stable workspace identity.
    pub fn workspace_accent(&self, _row: usize) -> Rgba {
        self.TEXT_DIM
    }

    /// Fill for a small control chip, such as the composer's pills. A chip
    /// sits on the pane surface, which is PANEL_BG while the pane owns focus
    /// and the brighter HEADER_BG while it does not, so one opaque paper sits
    /// a different distance from each. In the ristretto palette the focused
    /// pane is 4/255 away from USER_BG: the same pill read on an idle pane and
    /// disappeared on the focused one. Lifting the surface by a fixed share of
    /// the theme's own ink keeps the step on either surface, and on light
    /// palettes it recesses the chip instead of inverting the palette.
    pub fn chip_background(&self) -> Rgba {
        self.TEXT.opacity(0.12)
    }

    /// Stronger lift for a chip under the pointer, or opened by it.
    pub fn chip_active_background(&self) -> Rgba {
        self.TEXT.opacity(0.2)
    }

    /// Small secondary labels in the composer and its work overlay.
    pub fn composer_secondary(&self) -> Rgba {
        self.TEXT_DIM.blend(self.TEXT.opacity(0.2))
    }

    /// Raised paper for the active pane, recessed backing for its neighbors.
    /// Use palette roles rather than dimming the entire pane so transcript text,
    /// code, images, and controls retain their original contrast and opacity.
    pub fn panel_background(&self, focused: bool) -> Rgba {
        if focused {
            self.PANEL_BG
        } else {
            self.HEADER_BG
        }
    }

    /// The border of a card that is being pressed: the panel border pulled
    /// toward the accent as `press` goes from 0 to 1.
    pub fn pressed_border(&self, press: f32) -> Rgba {
        self.PANEL_BORDER.blend(self.ACCENT.opacity(0.45 * press))
    }

    /// Fill of one workspace minimap row. The active row's track is brighter,
    /// and both are tinted toward the row's `accent`.
    pub fn minimap_row_background(&self, active: bool, accent: Rgba) -> Rgba {
        let track = if active {
            self.MINIMAP_TRACK_ACTIVE
        } else {
            self.MINIMAP_TRACK
        };
        track.blend(accent.opacity(if active { 0.16 } else { 0.06 }))
    }

    /// Fill of a minimap row under the pointer.
    pub fn minimap_row_hover(&self, accent: Rgba) -> Rgba {
        self.MINIMAP_TRACK_ACTIVE.blend(accent.opacity(0.20))
    }

    /// A translucent canvas lets the desktop wallpaper through. Such palettes
    /// ask the compositor to blur behind the window so text stays legible.
    pub fn is_translucent(&self) -> bool {
        self.BG.a < 1.0
    }

    /// This palette with the user's configured fonts and color overrides
    /// applied. Unknown or malformed overrides are reported and skipped.
    pub(super) fn configured(mut self, appearance: &AppearanceConfig) -> Self {
        self.apply_fonts(appearance);
        for (role, value) in &appearance.colors {
            match parse_color(value) {
                Some(color) => self.set_color(role, color),
                None => eprintln!("ignoring invalid desktop color {role}={value:?}"),
            }
        }
        self
    }

    pub(super) fn apply_fonts(&mut self, appearance: &AppearanceConfig) {
        if let Some(font) = appearance.ui_font.as_deref() {
            self.FONT_UI = Box::leak(font.to_owned().into_boxed_str());
        }
        self.FONT_AI = appearance.ai_font.as_deref().map_or(self.FONT_UI, |font| {
            Box::leak(font.to_owned().into_boxed_str())
        });
        if let Some(font) = appearance.mono_font.as_deref() {
            self.FONT_MONO = Box::leak(font.to_owned().into_boxed_str());
        }
    }

    // Warm neutral: charcoal and stone surfaces, ivory type, restrained sandstone focus.
    pub(super) fn defaults() -> Self {
        Self {
            BG: rgb_c(0x1c1a18),
            CANVAS_DOT: rgba_c(0xe4ddd30a),
            PANEL_BG: rgb_c(0x25221f),
            PANEL_BORDER: rgb_c(0x403a34),
            PANEL_BORDER_FOCUS: rgb_c(0x87796b),
            PANEL_BORDER_IDLE: rgba_c(0x00000000),
            HEADER_BG: rgb_c(0x302b27),
            TEXT: rgb_c(0xe4ddd3),
            TEXT_DIM: rgb_c(0xa79d91),
            TEXT_USER: rgb_c(0xeee7de),
            ACCENT: rgb_c(0xb6a08a),
            ACCENT_DIM: rgba_c(0xb6a08a20),
            USER_ACCENT: rgb_c(0xc2b09d),
            AI_ACCENT: rgb_c(0xa9b09b),
            USER_BG: rgb_c(0x302b27),
            PROMPT_TINT_STRENGTH: 0.05,
            TOOL_BG: rgb_c(0x292521),
            TOOL_TEXT: rgb_c(0xa79d91),
            REASONING: rgb_c(0xa79d91),
            REASONING_BG: rgba_c(0xe4ddd306),
            TEXT_FAINT: rgb_c(0x9b9084),
            TOOL_BORDER: rgb_c(0x3b3530),
            ERROR_BG: rgba_c(0xff646414),
            CODE_BG: rgb_c(0x1c1a18),
            CODE_TEXT: rgb_c(0xd4d4d4),
            INLINE_CODE_BG: rgb_c(0x36302b),
            CODE_BORDER: rgb_c(0x3b3530),
            CODE_HEADER_BG: rgb_c(0x292521),
            CODE_GUTTER: rgb_c(0x92877c),
            CODE_KEYWORD: rgb_c(0x569cd6),
            CODE_STRING: rgb_c(0xce9178),
            CODE_COMMENT: rgb_c(0x6a9955),
            CODE_NUMBER: rgb_c(0xb5cea8),
            CODE_TYPE: rgb_c(0x4ec9b0),
            CODE_PUNCT: rgb_c(0xd4d4d4),
            CODE_FUNCTION: rgb_c(0xdcdcaa),
            CODE_VARIABLE: rgb_c(0x9cdcfe),
            CODE_CONTROL: rgb_c(0xc586c0),
            CODE_CONSTANT: rgb_c(0x4fc1ff),
            CODE_TAG: rgb_c(0x569cd6),
            CODE_ATTRIBUTE: rgb_c(0x9cdcfe),
            ACCENT_MUTED: rgb_c(0xa99a89),
            QUOTE_BG: rgba_c(0xe4ddd307),
            TABLE_STRIPE: rgba_c(0xe4ddd306),
            INPUT_BG: rgb_c(0x211e1b),
            INPUT_BORDER: rgb_c(0x51483f),
            CURSOR: rgb_c(0xeee7de),
            SELECTION: rgba_c(0x75665780),
            ERROR: rgb_c(0xe18c85),
            OK: rgb_c(0xa6b68e),
            WARN: rgb_c(0xd0b17d),
            HEADING: rgb_c(0xe4ddd3),
            LINK: rgb_c(0xc6b5a2),
            MINIMAP_TRACK: rgba_c(0xe4ddd306),
            MINIMAP_TRACK_ACTIVE: rgba_c(0xe4ddd30d),
            MINIMAP_VIEWPORT: rgba_c(0xb6a08a66),
            MINIMAP_PANEL: rgb_c(0x554c43),
            MINIMAP_PANEL_BUSY: rgb_c(0x786a5d),
            MINIMAP_BG: rgba_c(0x302b27e6),
            FONT_UI: platform_font(),
            FONT_AI: platform_font(),
            FONT_MONO: platform_font(),
        }
    }

    pub(super) fn set_color(&mut self, role: &str, color: Rgba) {
        match role.to_ascii_lowercase().replace('-', "_").as_str() {
            "bg" => self.BG = color,
            "canvas_dot" => self.CANVAS_DOT = color,
            "panel_bg" => self.PANEL_BG = color,
            "panel_border" => self.PANEL_BORDER = color,
            "panel_border_focus" => self.PANEL_BORDER_FOCUS = color,
            "panel_border_idle" => self.PANEL_BORDER_IDLE = color,
            "header_bg" => self.HEADER_BG = color,
            "text" => self.TEXT = color,
            "text_dim" => self.TEXT_DIM = color,
            "text_user" => self.TEXT_USER = color,
            "accent" => self.ACCENT = color,
            "accent_dim" => self.ACCENT_DIM = color,
            "user_accent" => self.USER_ACCENT = color,
            "ai_accent" => self.AI_ACCENT = color,
            "user_bg" => self.USER_BG = color,
            "tool_bg" => self.TOOL_BG = color,
            "tool_text" => self.TOOL_TEXT = color,
            "reasoning" => self.REASONING = color,
            "reasoning_bg" => self.REASONING_BG = color,
            "text_faint" => self.TEXT_FAINT = color,
            "tool_border" => self.TOOL_BORDER = color,
            "error_bg" => self.ERROR_BG = color,
            "code_bg" => self.CODE_BG = color,
            "code_text" => self.CODE_TEXT = color,
            "inline_code_bg" => self.INLINE_CODE_BG = color,
            "code_border" => self.CODE_BORDER = color,
            "code_header_bg" => self.CODE_HEADER_BG = color,
            "code_gutter" => self.CODE_GUTTER = color,
            "code_keyword" => self.CODE_KEYWORD = color,
            "code_string" => self.CODE_STRING = color,
            "code_comment" => self.CODE_COMMENT = color,
            "code_number" => self.CODE_NUMBER = color,
            "code_type" => self.CODE_TYPE = color,
            "code_punct" => self.CODE_PUNCT = color,
            "code_function" => self.CODE_FUNCTION = color,
            "code_variable" => self.CODE_VARIABLE = color,
            "code_control" => self.CODE_CONTROL = color,
            "code_constant" => self.CODE_CONSTANT = color,
            "code_tag" => self.CODE_TAG = color,
            "code_attribute" => self.CODE_ATTRIBUTE = color,
            "accent_muted" => self.ACCENT_MUTED = color,
            "quote_bg" => self.QUOTE_BG = color,
            "table_stripe" => self.TABLE_STRIPE = color,
            "input_bg" => self.INPUT_BG = color,
            "input_border" => self.INPUT_BORDER = color,
            "cursor" => self.CURSOR = color,
            "selection" => self.SELECTION = color,
            "error" => self.ERROR = color,
            "ok" => self.OK = color,
            "warn" => self.WARN = color,
            "heading" => self.HEADING = color,
            "link" => self.LINK = color,
            "minimap_track" => self.MINIMAP_TRACK = color,
            "minimap_track_active" => self.MINIMAP_TRACK_ACTIVE = color,
            "minimap_viewport" => self.MINIMAP_VIEWPORT = color,
            "minimap_panel" => self.MINIMAP_PANEL = color,
            "minimap_panel_busy" => self.MINIMAP_PANEL_BUSY = color,
            "minimap_bg" => self.MINIMAP_BG = color,
            _ => eprintln!("ignoring unknown desktop color role {role:?}"),
        }
    }

    /// The palette `amount` of the way from `self` to `to`, channel by
    /// channel. Fonts stay `self`'s: a fade only moves colors.
    pub(super) fn mixed(&self, to: &Self, amount: f32) -> Self {
        if amount <= 0.0 {
            return self.clone();
        }
        if amount >= 1.0 {
            return to.clone();
        }
        let mut result = self.clone();
        result.PROMPT_TINT_STRENGTH = self.PROMPT_TINT_STRENGTH
            + (to.PROMPT_TINT_STRENGTH - self.PROMPT_TINT_STRENGTH) * amount;
        let mix = |a: Rgba, b: Rgba| Rgba {
            r: a.r + (b.r - a.r) * amount,
            g: a.g + (b.g - a.g) * amount,
            b: a.b + (b.b - a.b) * amount,
            a: a.a + (b.a - a.a) * amount,
        };
        macro_rules! colors { ($($field:ident),+ $(,)?) => { $(result.$field = mix(self.$field, to.$field);)+ }; }
        colors!(
            BG,
            CANVAS_DOT,
            PANEL_BG,
            PANEL_BORDER,
            PANEL_BORDER_FOCUS,
            PANEL_BORDER_IDLE,
            HEADER_BG,
            TEXT,
            TEXT_DIM,
            TEXT_USER,
            ACCENT,
            ACCENT_DIM,
            USER_ACCENT,
            AI_ACCENT,
            USER_BG,
            TOOL_BG,
            TOOL_TEXT,
            REASONING,
            REASONING_BG,
            TEXT_FAINT,
            TOOL_BORDER,
            ERROR_BG,
            CODE_BG,
            CODE_TEXT,
            INLINE_CODE_BG,
            CODE_BORDER,
            CODE_HEADER_BG,
            CODE_GUTTER,
            CODE_KEYWORD,
            CODE_STRING,
            CODE_COMMENT,
            CODE_NUMBER,
            CODE_TYPE,
            CODE_PUNCT,
            CODE_FUNCTION,
            CODE_VARIABLE,
            CODE_CONTROL,
            CODE_CONSTANT,
            CODE_TAG,
            CODE_ATTRIBUTE,
            ACCENT_MUTED,
            QUOTE_BG,
            TABLE_STRIPE,
            INPUT_BG,
            INPUT_BORDER,
            CURSOR,
            SELECTION,
            ERROR,
            OK,
            WARN,
            HEADING,
            LINK,
            MINIMAP_TRACK,
            MINIMAP_TRACK_ACTIVE,
            MINIMAP_VIEWPORT,
            MINIMAP_PANEL,
            MINIMAP_PANEL_BUSY,
            MINIMAP_BG
        );
        result
    }
}

#[cfg(target_os = "macos")]
pub(super) const fn platform_font() -> &'static str {
    "Menlo"
}
#[cfg(not(target_os = "macos"))]
pub(super) const fn platform_font() -> &'static str {
    "JetBrainsMono Nerd Font"
}

pub(super) fn parse_color(value: &str) -> Option<Rgba> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let raw = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(rgb_c(raw)),
        8 => Some(rgba_c(raw)),
        _ => None,
    }
}

pub fn to_hsla(color: Rgba) -> Hsla {
    color.into()
}

pub(super) const fn rgb_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

pub(super) const fn rgba_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 24) & 0xff) as f32 / 255.0,
        g: ((hex >> 16) & 0xff) as f32 / 255.0,
        b: ((hex >> 8) & 0xff) as f32 / 255.0,
        a: (hex & 0xff) as f32 / 255.0,
    }
}
