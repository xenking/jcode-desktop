//! The rows the command menu offers for what is typed.
use super::*;

#[allow(dead_code)]
pub(super) fn command_suggestions(input: &str, models: &[String]) -> Vec<CommandSuggestion> {
    command_suggestions_with_efforts(input, models, EffortMenu::default())
}

/// What the `/effort` menu offers: the serving model's ladder and its level.
#[derive(Clone, Copy, Default)]
pub(super) struct EffortMenu<'a> {
    pub(super) ladder: Option<&'a [&'static str]>,
    pub(super) current: Option<&'a str>,
}

pub(super) fn command_suggestions_with_efforts(
    input: &str,
    models: &[String],
    efforts: EffortMenu<'_>,
) -> Vec<CommandSuggestion> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains('\n') {
        return Vec::new();
    }
    // Keep the command in the composer, so typing `/models` remains possible.
    // Model rows replace the command rows in the same suggestion area.
    if trimmed == "/model" || trimmed.starts_with("/model ") {
        let query = trimmed
            .split_once(' ')
            .map(|(_, query)| query.trim().to_ascii_lowercase())
            .unwrap_or_default();
        return models
            .iter()
            .filter(|model| query.is_empty() || model.to_ascii_lowercase().contains(&query))
            .map(|model| CommandSuggestion {
                value: format!("/model {model}"),
                help: "Switch this session".into(),
                active_choice: false,
                detail: None,
                header: None,
                toggle: None,
                context_action: None,
                action: None,
            })
            .collect();
    }
    if trimmed == "/effort" || trimmed.starts_with("/effort ") {
        let query = trimmed
            .split_once(' ')
            .map(|(_, query)| query.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let fallback = crate::effort::ladder(None, None);
        let ladder = efforts.ladder.unwrap_or(&fallback);
        return ladder
            .iter()
            .copied()
            .filter(|effort| query.is_empty() || effort.contains(&query))
            .map(|effort| CommandSuggestion {
                value: format!("/effort {effort}"),
                help: if effort == "swarm" {
                    "Low-effort root that fans work out to workers".into()
                } else {
                    "Set reasoning effort".into()
                },
                active_choice: efforts.current == Some(effort),
                detail: None,
                header: None,
                toggle: None,
                context_action: None,
                action: None,
            })
            .collect();
    }
    let query = trimmed.to_ascii_lowercase();
    let mut commands = registered_command_entries()
        .filter_map(|(command, help)| {
            let command_lower = command.to_ascii_lowercase();
            let help_lower = help.to_ascii_lowercase();
            (command_lower.starts_with(&query)
                || command_lower.contains(query.trim_start_matches('/'))
                || help_lower.contains(query.trim_start_matches('/')))
            .then_some((command, help, command_lower.starts_with(&query)))
        })
        .collect::<Vec<_>>();
    commands.sort_by_key(|(name, _, prefix)| (name.to_ascii_lowercase() != query, !prefix));
    commands
        .into_iter()
        .map(|(value, help, _)| CommandSuggestion {
            value: value.into(),
            help: help.into(),
            active_choice: false,
            detail: None,
            header: None,
            toggle: None,
            context_action: None,
            action: None,
        })
        .collect()
}

pub(super) fn mode_suggestions(
    input: &str,
    skills: &[(String, String)],
    active: &[String],
    catalog: &crate::mode_catalog::ModeCatalog,
) -> Vec<CommandSuggestion> {
    let query = input
        .strip_prefix("/mode")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut group = None;
    let mut push = |section: &'static str, name: &str, help: &str| {
        if !name.to_ascii_lowercase().contains(&query) {
            return;
        }
        let header = (group.replace(section) != Some(section)).then(|| section.into());
        rows.push(CommandSuggestion {
            active_choice: false,
            value: format!("/mode {name}"),
            help: help.to_owned(),
            detail: None,
            header,
            toggle: None,
            context_action: None,
            action: None,
        });
    };
    for name in active {
        let help = skills
            .iter()
            .find(|(skill, _)| skill == name)
            .map_or("Pinned in this session", |(_, help)| help);
        push("Active", name, help);
    }
    for (section, modes) in [("Modes", true), ("Skills", false)] {
        for (name, help) in skills {
            if name != "off"
                && !active.contains(name)
                && crate::mode_catalog::is_mode(name, catalog.get(name)) == modes
            {
                push(section, name, help);
            }
        }
    }
    if !active.is_empty() {
        push("Actions", "off", "Clear all active modes");
    }
    rows
}

pub(super) fn accepted_command_submission(
    suggestions: &[CommandSuggestion],
    selection: usize,
) -> Option<String> {
    let suggestion = suggestions.get(selection.min(suggestions.len().saturating_sub(1)))?;
    // Matching includes descriptions, not only command-name prefixes. Enter
    // must select the highlighted result for either kind of search.
    (suggestion.toggle.is_none() && suggestion.action.is_none()).then(|| suggestion.value.clone())
}
