use std::collections::HashMap;

use gpui::{Rgba, Styled, Svg, px, svg};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModeMetadata {
    pub icon: Option<String>,
    pub mode: Option<bool>,
}

pub(crate) type ModeCatalog = HashMap<String, ModeMetadata>;

pub(crate) fn is_mode(name: &str, metadata: Option<&ModeMetadata>) -> bool {
    metadata
        .and_then(|item| item.mode)
        .unwrap_or_else(|| jcode_base::skill::name_implies_mode(name))
}

macro_rules! mode_icons {
    ($($variant:ident => $name:literal, $path:literal;)*) => {
        #[derive(Clone, Copy)]
        pub(crate) enum ModeIcon { $($variant,)* }

        impl ModeIcon {
            fn named(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub(crate) fn name(self) -> &'static str {
                match self { $(Self::$variant => $name,)* }
            }

            /// The icon's own colour in a picker row, from the active palette.
            pub(crate) fn tint(self, theme: &crate::theme::Palette) -> Rgba {
                match self {
                    Self::Selfdev => theme.CODE_FUNCTION,
                    Self::Swarm => theme.CODE_KEYWORD,
                    Self::Crown | Self::Plan => theme.WARN,
                    Self::Moon => theme.CODE_TYPE,
                    Self::Trash | Self::Bug => theme.ERROR,
                    Self::Todo | Self::Check | Self::Help => theme.OK,
                    Self::Pencil | Self::Layers => theme.CODE_STRING,
                    Self::Cloud | Self::Monitor | Self::Zap => theme.CODE_FUNCTION,
                    Self::Search | Self::Document | Self::Book | Self::Compass => theme.CODE_CONSTANT,
                    Self::Multitask => theme.CODE_CONTROL,
                    Self::Flask => theme.CODE_TYPE,
                    Self::Bulb => theme.CODE_NUMBER,
                    Self::Sparkles => theme.CODE_KEYWORD,
                    Self::Terminal
                    | Self::Star
                    | Self::Generic
                    | Self::Paperclip
                    | Self::Plug
                    | Self::ChevronRight
                    | Self::ChevronLeft => theme.TEXT_DIM,
                }
            }

            pub(crate) fn render(self, size: f32, ink: Rgba) -> Svg {
                let data = match self {
                    $(Self::$variant => include_bytes!(concat!("../../../../../../assets/icons/", $path)).as_slice(),)*
                };
                svg().data(data).size(px(size)).flex_none().text_color(ink)
            }
        }
    };
}

mode_icons! {
    Generic => "generic", "tools/generic.svg";
    Selfdev => "selfdev", "tools/selfdev.svg";
    Swarm => "swarm", "swarm.svg";
    Crown => "crown", "crown.svg";
    Moon => "moon", "moon.svg";
    Trash => "trash", "trash.svg";
    Todo => "todo", "tools/todo.svg";
    Pencil => "pencil", "pencil.svg";
    Cloud => "cloud", "cloud.svg";
    Search => "search", "tools/session_search.svg";
    Document => "document", "tools/read.svg";
    Book => "book", "tools/jcode_docs.svg";
    Terminal => "terminal", "tools/bash.svg";
    Monitor => "monitor", "computer.svg";
    Star => "star", "tools/skill_manage.svg";
    Check => "check", "swarm/check.svg";
    Plan => "plan", "modes/plan.svg";
    Bug => "bug", "modes/bug.svg";
    Multitask => "multitask", "modes/multitask.svg";
    Help => "help", "modes/help.svg";
    Paperclip => "paperclip", "modes/paperclip.svg";
    Plug => "plug", "plug.svg";
    Zap => "zap", "modes/zap.svg";
    Compass => "compass", "modes/compass.svg";
    Layers => "layers", "modes/layers.svg";
    Flask => "flask", "modes/flask.svg";
    Bulb => "bulb", "modes/bulb.svg";
    Sparkles => "sparkles", "modes/sparkles.svg";
    ChevronRight => "chevron-right", "modes/chevron-right.svg";
    ChevronLeft => "chevron-left", "modes/chevron-left.svg";
}

/// Glyphs a mode with no known meaning is spread over. Each carries its own
/// colour, so a name always lands on the same icon and the same hue.
const SPREAD: [ModeIcon; 6] = [
    ModeIcon::Sparkles,
    ModeIcon::Zap,
    ModeIcon::Compass,
    ModeIcon::Layers,
    ModeIcon::Flask,
    ModeIcon::Bulb,
];

/// FNV-1a: stable across runs and releases, unlike `DefaultHasher`.
fn stable_hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte.to_ascii_lowercase())).wrapping_mul(0x0100_0000_01b3)
    })
}

pub(crate) fn icon(name: &str, metadata: Option<&ModeMetadata>) -> ModeIcon {
    use ModeIcon::*;
    if let Some(override_name) = metadata.and_then(|item| item.icon.as_deref()) {
        return ModeIcon::named(override_name.trim()).unwrap_or(Generic);
    }
    match name {
        "selfdev" => Selfdev,
        "swarm" => Swarm,
        "poteto-mode" => Crown,
        "off" => Trash,
        _ => name
            .split('-')
            .find_map(|word| match word {
                "plan" | "planning" | "planner" => Some(Plan),
                "debug" | "bug" | "bugfix" => Some(Bug),
                "multitask" | "parallel" | "orchestrate" | "orchestrator" => Some(Multitask),
                "ask" | "question" | "questions" | "qa" => Some(Help),
                "quiet" | "brief" | "silent" => Some(Moon),
                "test" | "tdd" | "verify" | "verification" | "audit" => Some(Todo),
                "write" | "writing" | "doc" | "docs" | "transcript" => Some(Pencil),
                "deploy" | "sync" | "migrate" => Some(Cloud),
                "search" | "research" | "diagnose" => Some(Search),
                _ => None,
            })
            .unwrap_or_else(|| SPREAD[(stable_hash(name) % SPREAD.len() as u64) as usize]),
    }
}

pub(crate) fn pinned_names(data: &serde_json::Value, legacy_key: &str) -> Vec<String> {
    if let Some(pinned) = data["pinned_skills"].as_array() {
        return pinned
            .iter()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect();
    }
    data[legacy_key]
        .as_str()
        .map(str::to_owned)
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_icon_metadata_resolves_to_bundled_assets_never_to_text() {
        assert_eq!(icon("poteto-mode", None).name(), "crown");
        assert_ne!(icon("custom-mode", None).name(), "generic");
        for (raw, expected) in [
            (" crown ", "crown"),
            ("terminal", "terminal"),
            ("crown text", "generic"),
            ("✎", "generic"),
            ("../crown.svg", "generic"),
            ("", "generic"),
        ] {
            let metadata = ModeMetadata {
                icon: Some(raw.into()),
                mode: None,
            };
            assert_eq!(
                icon("poteto-mode", Some(&metadata)).name(),
                expected,
                "{raw}"
            );
        }
    }

    #[test]
    fn well_known_modes_get_their_own_glyph_and_colour() {
        let theme = crate::theme::Theme::global();
        let looks: Vec<_> = ["plan", "debug", "multitask", "ask", "selfdev", "swarm", "quiet", "poteto-mode"]
            .into_iter()
            .map(|name| {
                let icon = icon(name, None);
                (icon.name(), icon.tint(theme))
            })
            .collect();
        let names: Vec<_> = looks.iter().map(|look| look.0).collect();
        assert_eq!(names, ["plan", "bug", "multitask", "help", "selfdev", "swarm", "moon", "crown"]);
        for (index, look) in looks.iter().enumerate() {
            assert!(
                looks[index + 1..].iter().all(|other| other.0 != look.0),
                "{} shares its glyph",
                look.0
            );
        }
        // The four modes the picker leads with must also read apart by colour.
        for pair in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
            assert_ne!(looks[pair.0].1, looks[pair.1].1, "{pair:?}");
        }
    }

    #[test]
    fn unknown_modes_spread_over_glyphs_and_never_change() {
        let names = ["alpha", "beta-flow", "gamma-ray", "delta", "epsilon-run", "zeta", "eta-pass", "theta"];
        let first: Vec<_> = names.iter().map(|name| icon(name, None).name()).collect();
        let again: Vec<_> = names.iter().map(|name| icon(&name.to_uppercase(), None).name()).collect();
        assert_eq!(first, again, "case does not move a mode to another icon");
        let mut distinct = first.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(distinct.len() >= 3, "all fell on {distinct:?}");
        assert!(first.iter().all(|name| *name != "generic"));
    }
}
