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

            pub(crate) fn render(self, size: f32, ink: Rgba) -> Svg {
                let data = match self {
                    $(Self::$variant => include_bytes!(concat!("../../../assets/icons/", $path)).as_slice(),)*
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
                "test" | "tdd" | "verify" | "verification" | "audit" => Some(Todo),
                "write" | "writing" | "doc" | "docs" | "transcript" => Some(Pencil),
                "deploy" | "sync" | "migrate" => Some(Cloud),
                "search" | "research" | "diagnose" => Some(Search),
                _ => None,
            })
            .unwrap_or(Generic),
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
        assert_eq!(icon("custom-mode", None).name(), "generic");
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
}
