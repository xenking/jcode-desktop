//! The brand mark for the service a session runs on.
//!
//! A worker row has one line for its identity, so the provider is a 12px logo
//! rather than a word: the model family's own mark when it is recognised
//! (OpenAI's for a GPT, Anthropic's for a Claude), else the routing service's
//! (OpenRouter, OpenCode), else a lettermark. Shared by the sidebar rows, the
//! composer workers list and the swarm cards so all three agree.

use gpui::{AnyElement, IntoElement, Rgba, div, prelude::*, px};

/// Logo id (see `accounts::logo`) for a worker's route and model.
///
/// The model's maker wins: an OpenAI model served through OpenRouter is still
/// an OpenAI session. The route only names the mark when the family is
/// unknown.
pub(crate) fn logo_id(provider: Option<&str>, model: Option<&str>) -> &'static str {
    if let Some(family) = model.and_then(family_logo) {
        return family;
    }
    provider_logo(provider.unwrap_or_default(), "")
}

/// Logo id (see `accounts::logo`) for the service a route goes through.
pub(crate) fn provider_logo(provider: &str, api_method: &str) -> &'static str {
    let haystack = format!("{provider} {api_method}").to_ascii_lowercase();
    for (needle, logo) in [
        ("openrouter", "openrouter"),
        ("copilot", "copilot"),
        ("cursor", "cursor"),
        ("bedrock", "bedrock"),
        ("azure", "azure"),
        ("antigravity", "antigravity"),
        ("jcode", "jcode"),
        ("anthropic", "anthropic-api"),
        ("claude", "anthropic-api"),
        ("openai", "openai"),
        ("chatgpt", "openai"),
        ("gemini", "gemini"),
        ("code-assist", "gemini"),
        ("google", "gemini"),
        ("grok", "xai"),
        ("xai", "xai"),
        ("mistral", "mistral"),
        ("deepseek", "deepseek"),
        ("groq", "groq"),
        ("ollama", "ollama"),
        ("lmstudio", "lmstudio"),
    ] {
        if haystack.contains(needle) {
            return logo;
        }
    }
    ""
}

/// `oauth`, `api_key`, or a route id such as `claude-oauth`, phrased for the
/// account label.
pub(crate) fn human_auth_method(method: &str) -> String {
    let method = method.to_lowercase();
    if method.contains("oauth") {
        "oauth".to_string()
    } else if method.contains("api-key") || method.contains("api_key") || method.ends_with("-api") {
        "api key".to_string()
    } else {
        method
    }
}

/// The model family's mark, from the leading word of the model id.
fn family_logo(model: &str) -> Option<&'static str> {
    let bare = model.trim().rsplit('/').next().unwrap_or_default();
    let vendor = bare.to_ascii_lowercase();
    let first = vendor.split(['-', '.', '_', ':']).next().unwrap_or_default();
    Some(match first {
        "claude" | "anthropic" => "anthropic-api",
        "gpt" | "o1" | "o3" | "o4" | "chatgpt" | "openai" => "openai",
        "gemini" | "google" | "gemma" => "gemini",
        "deepseek" => "deepseek",
        "mistral" | "mistralai" | "codestral" => "mistral",
        "grok" | "xai" => "xai",
        "kimi" | "moonshotai" | "moonshot" => "kimi",
        "qwen" | "qwen3" => "alibaba-coding-plan",
        "zai" | "zhipu" | "glm" => "zai",
        "minimax" => "minimax",
        "llama" | "meta" => "",
        _ => return None,
    })
}

/// The mark itself, or a lettermark when the service has no logo of ours.
pub(crate) fn render(
    provider: Option<&str>,
    model: Option<&str>,
    size: f32,
    color: Rgba,
) -> AnyElement {
    let logo = logo_id(provider, model);
    match crate::accounts::logo(logo) {
        Some(bytes) => gpui::svg()
            .data(bytes)
            .size(px(size))
            .flex_none()
            .text_color(color)
            .into_any_element(),
        None => div()
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.62))
            .text_color(color)
            .child(crate::accounts::lettermark(&fallback_name(provider, model)))
            .into_any_element(),
    }
}

/// What a lettermark spells: the provider key, else the model's first word.
fn fallback_name(provider: Option<&str>, model: Option<&str>) -> String {
    provider
        .map(|provider| provider.trim_end_matches("-oauth").trim_end_matches("-api"))
        .filter(|provider| !provider.is_empty())
        .or_else(|| {
            model
                .and_then(|model| model.split(['-', '/']).next())
                .filter(|word| !word.is_empty())
        })
        .unwrap_or("?")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_family_mark_wins_over_the_route() {
        assert_eq!(logo_id(Some("openrouter"), Some("openai/gpt-5.6")), "openai");
        assert_eq!(
            logo_id(Some("claude-oauth"), Some("claude-opus-5-5")),
            "anthropic-api"
        );
        assert_eq!(
            logo_id(Some("opencode-go"), Some("deepseek-v4.1-flash")),
            "deepseek"
        );
        // The route names the mark only when the family is unknown.
        assert_eq!(logo_id(Some("openrouter"), Some("widget-1")), "openrouter");
    }

    #[test]
    fn an_unknown_route_falls_back_to_a_lettermark_name() {
        assert_eq!(logo_id(Some("mystery"), Some("widget-1")), "");
        assert_eq!(fallback_name(Some("mystery-oauth"), None), "mystery");
        assert_eq!(fallback_name(None, Some("widget-1")), "widget");
        assert_eq!(fallback_name(None, None), "?");
    }

    #[test]
    fn auth_methods_read_as_oauth_or_api_key() {
        assert_eq!(human_auth_method("oauth"), "oauth");
        assert_eq!(human_auth_method("api_key"), "api key");
        assert_eq!(human_auth_method("claude-api"), "api key");
    }
}
