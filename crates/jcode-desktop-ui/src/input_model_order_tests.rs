//! Picker ordering fixture. The ids are real `jcode model list` shapes.
use super::model_menu::{ModelDetails, from_routes, grouped_rows};
use super::*;
use jcode_sdk::ModelRouteInfo;

fn route(model: &str, provider: &str, method: &str, count: u64) -> ModelRouteInfo {
    ModelRouteInfo {
        model: model.into(),
        provider: provider.into(),
        api_method: method.into(),
        available: true,
        detail: String::new(),
        // Usage must not decide order: later entries are the most used.
        usage: Some(jcode_sdk::ModelUsage {
            count,
            last_used_unix_secs: Some(count),
            tracking_started_unix_secs: Some(1),
            selection_count: 0,
            last_selected_unix_secs: None,
        }),
    }
}

fn fixture() -> Vec<ModelRouteInfo> {
    let mut routes = Vec::new();
    // Deliberately shuffled, with the newest generation least used.
    for (n, model) in [
        "gpt-5.5",
        "gpt-6-sol",
        "gpt-5-mini",
        "gpt-6.1-sol",
        "gpt-5.6-sol",
        "gpt-6.1-sol-pro",
        "gpt-5.3-codex",
        "gpt-5.5-pro",
        "gpt-6-astra",
    ]
    .into_iter()
    .enumerate()
    {
        routes.push(route(model, "OpenAI", "openai-oauth", 100 - n as u64));
    }
    for (n, model) in [
        "claude-opus-4-8",
        "claude-fable-5-2",
        "claude-sonnet-5-5",
        "claude-haiku-4-5-20251001",
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-haiku-5-5",
        "claude-fable-5-1",
        "claude-opus-4-6",
    ]
    .into_iter()
    .enumerate()
    {
        routes.push(route(model, "Anthropic", "claude-oauth", 50 - n as u64));
    }
    for (n, model) in [
        "~openai/gpt-sol-latest",
        "openai/gpt-6.1-sol:batch",
        "google/gemini-4-carbon:batch",
        "anthropic/claude-fable-5.2:batch",
        "openai/gpt-6.1-sol",
        "google/gemini-4-carbon",
        "anthropic/claude-opus-5.5",
        "anthropic/claude-fable-5.2",
        "~anthropic/claude-opus-latest",
    ]
    .into_iter()
    .enumerate()
    {
        routes.push(route(model, "OpenRouter", "openrouter", n as u64));
    }
    routes
}

/// Models per group header, in displayed order, with every group expanded.
fn expanded(details: &HashMap<String, ModelDetails>) -> Vec<(String, Vec<String>)> {
    let models: Vec<String> = details.keys().cloned().collect();
    let all_open: HashSet<String> = models
        .iter()
        .map(|model| model_menu::group_key(model, details))
        .collect();
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for row in grouped_rows(&models, details, None, &all_open, "") {
        let Some(spec) = row.value.strip_prefix("/model ") else {
            continue;
        };
        let name = details[spec].model.clone();
        match row.header {
            Some(header) => groups.push((header, vec![name])),
            None => groups.last_mut().unwrap().1.push(name),
        }
    }
    groups
}

#[test]
fn picker_groups_by_provider_then_family_newest_first() {
    let details = from_routes(&fixture());
    let groups = expanded(&details);
    assert_eq!(
        groups
            .iter()
            .map(|(header, _)| header.as_str())
            .collect::<Vec<_>>(),
        [
            "Anthropic · Signed in",
            "OpenAI · Signed in",
            "OpenRouter · API key"
        ],
        "providers are alphabetical and stable, never usage-ordered"
    );
    assert_eq!(
        groups[0].1,
        [
            "claude-haiku-5-5",
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-fable-5-2",
            "claude-fable-5-1",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-6",
            "claude-haiku-4-5-20251001",
        ]
    );
    assert_eq!(
        groups[1].1,
        [
            "gpt-6.1-sol",
            "gpt-6.1-sol-pro",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-5.6-sol",
            "gpt-5.5",
            "gpt-5.5-pro",
            "gpt-5.3-codex",
            "gpt-5-mini",
        ]
    );
    assert_eq!(
        groups[2].1,
        [
            "anthropic/claude-opus-5.5",
            "anthropic/claude-fable-5.2",
            "anthropic/claude-fable-5.2:batch",
            "~anthropic/claude-opus-latest",
            "google/gemini-4-carbon",
            "google/gemini-4-carbon:batch",
            "openai/gpt-6.1-sol",
            "openai/gpt-6.1-sol:batch",
            "~openai/gpt-sol-latest",
        ],
        "batch variants follow their base and ~latest aliases trail the family"
    );
}

#[test]
fn collapsed_groups_lead_with_base_models_not_variants_or_aliases() {
    let details = from_routes(&fixture());
    let models: Vec<String> = details.keys().cloned().collect();
    let rows = grouped_rows(&models, &details, None, &HashSet::new(), "");
    let router: Vec<_> = rows
        .iter()
        .skip_while(|row| row.header.as_deref() != Some("OpenRouter · API key"))
        .filter_map(|row| row.value.strip_prefix("/model "))
        .map(|spec| details[spec].model.as_str())
        .collect();
    assert_eq!(
        router,
        [
            "anthropic/claude-opus-5.5",
            "anthropic/claude-fable-5.2",
            "google/gemini-4-carbon"
        ]
    );
}

#[test]
fn ordering_ignores_input_order_and_usage() {
    let routes = fixture();
    let forward = expanded(&from_routes(&routes));
    let mut reversed = routes.clone();
    reversed.reverse();
    for route in &mut reversed {
        if let Some(usage) = route.usage.as_mut() {
            usage.count = 0;
        }
    }
    assert_eq!(forward, expanded(&from_routes(&reversed)));
}
