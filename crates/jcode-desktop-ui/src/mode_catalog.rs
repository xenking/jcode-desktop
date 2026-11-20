use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModeMetadata {
    pub icon: Option<String>,
    pub mode: Option<bool>,
}

pub(crate) type ModeCatalog = HashMap<String, ModeMetadata>;

pub(crate) fn is_mode(name: &str, metadata: Option<&ModeMetadata>) -> bool {
    metadata
        .and_then(|item| item.mode)
        .unwrap_or_else(|| matches!(name, "selfdev" | "swarm") || name.ends_with("-mode"))
}

pub(crate) fn icon<'a>(name: &str, metadata: Option<&'a ModeMetadata>) -> &'a str {
    if let Some(icon) = metadata
        .and_then(|item| item.icon.as_deref())
        .map(str::trim)
        && !icon.is_empty()
        && !icon.chars().any(char::is_control)
        && icon.chars().count() <= 16
    {
        return icon;
    }
    match name {
        "selfdev" => "⚙",
        "swarm" => "◈",
        "poteto-mode" => "🥔",
        "off" => "○",
        name if is_mode(name, metadata) => "◉",
        name if name.contains("test") || name.contains("verify") || name.contains("audit") => "⊙",
        name if name.contains("write") || name.contains("doc") || name.contains("transcript") => {
            "✎"
        }
        name if name.contains("deploy") || name.contains("sync") || name.contains("migrate") => "⇄",
        _ => "◇",
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
