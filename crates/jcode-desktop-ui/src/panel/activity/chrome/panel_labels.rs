//! Small text formatting shared by the transcript, footer and sidebar.
use super::*;

pub(super) fn short_id(session_id: &str) -> String {
    let tail: String = session_id
        .chars()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("session {tail}")
}

/// Which speaker a row belongs to, or None for rows that carry no caption.
pub(super) fn role_of(item: &Item) -> Option<&'static str> {
    match item {
        Item::User(_) => Some("you"),
        Item::Tool {
            name, done: true, ..
        } if name == "ask" => Some("you"),
        Item::Assistant(_) | Item::AssistantOutput { .. } => Some("jcode"),
        Item::AgentMessage { .. }
        | Item::SideQuestion(_)
        | Item::Image(_)
        | Item::ResponseStats(_)
        | Item::Reasoning(_)
        | Item::Tool { .. }
        | Item::BackgroundTask { .. }
        | Item::Todos(_)
        | Item::Stopped(_)
        | Item::SwarmWorkerNotice(_)
        | Item::ProviderGuardrailRetry { .. }
        | Item::Compaction(_)
        | Item::Memory(_)
        | Item::CacheMiss(_)
        | Item::Applet(_)
        | Item::Error(_)
        | Item::RuntimeError { .. } => None,
    }
}

impl Panel {
    /// Provider a bare `/reset` targets: Claude for Anthropic sessions,
    /// otherwise OpenAI, which matches the TUI default.
    pub(super) fn reset_provider(&self) -> crate::accounts::ResetProvider {
        let is_claude = |value: Option<&str>| {
            value.is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("anthropic") || value.contains("claude")
            })
        };
        if is_claude(self.provider.as_deref())
            || (self.provider.is_none() && is_claude(self.model.as_deref()))
        {
            crate::accounts::ResetProvider::Claude
        } else {
            crate::accounts::ResetProvider::OpenAi
        }
    }
}

/// The credential route serving `model`, phrased for humans.
/// Fallback for daemons that do not report the resolved credential. A model
/// can have both an API key and an OAuth route, so the first match is not the
/// serving one: prefer an available route, and OAuth over API key, matching
/// the daemon's own auto rule.
pub(super) fn auth_method_for_model(
    model: Option<&str>,
    routes: &[jcode_sdk::ModelRouteInfo],
) -> Option<String> {
    let model = model?;
    let route = routes
        .iter()
        .filter(|route| route.model == model)
        .max_by_key(|route| {
            (
                route.available,
                route.api_method.to_ascii_lowercase().contains("oauth"),
            )
        })?;
    Some(human_auth_method(&route.api_method))
}

pub(super) fn available_model_names(routes: &[jcode_sdk::ModelRouteInfo]) -> Vec<String> {
    let mut models: Vec<String> = routes
        .iter()
        .filter(|route| route.available)
        .map(|route| route.model.clone())
        .collect();
    models.sort();
    models.dedup();
    models
}

/// Label the account control with the current provider and credential method.
/// Keep account setup discoverable before runtime identity arrives.
/// "4s", "1m 12s", "1h 03m". Whole seconds keep the row width stable.
pub(super) fn format_tool_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, secs % 3600 / 60)
    }
}

pub(super) fn serving_account_label(
    provider: Option<&str>,
    auth_method: Option<&str>,
    account: Option<&str>,
) -> String {
    let method = account_method_label(provider, auth_method);
    match account.map(str::trim).filter(|account| !account.is_empty()) {
        Some(account) => format!("{method} · {account}"),
        None => method,
    }
}

pub(super) fn account_method_label(provider: Option<&str>, auth_method: Option<&str>) -> String {
    let provider = provider
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(pretty_provider_name);
    let method = auth_method
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(pretty_auth_method);
    match (provider, method) {
        (Some(provider), Some(method)) => format!("{provider} · {method}"),
        (Some(provider), None) => provider,
        (None, Some(method)) => method,
        (None, None) => "Accounts".into(),
    }
}

pub(super) fn pretty_auth_method(method: &str) -> String {
    match method.to_ascii_lowercase().as_str() {
        "oauth" => "OAuth".into(),
        "api key" | "api-key" | "api_key" => "API key".into(),
        other => other.to_string(),
    }
}

/// Shorten a home-relative path for the footer.
#[cfg(test)]
pub(super) fn compact_dir(path: &str) -> String {
    match crate::platform_dirs::tilde(path) {
        home if home == "~" => "~ (home)".to_string(),
        shortened => shortened,
    }
}

/// Use the same model capacity catalog as the CLI instead of desktop-only guesses.
pub(super) fn context_window_for_model(model: &str) -> Option<u64> {
    jcode_base::provider::context_limit_for_model_with_provider(model, None).map(|n| n as u64)
}

/// Match the TUI's output-token estimate and thresholds, using desktop theme
/// colors. The tilde distinguishes the estimate from provider-billed usage.
pub(super) fn tool_output_token_badge(output: &str) -> (String, gpui::Rgba) {
    use jcode_base::message::{TokenFamily, estimate_text};
    use jcode_core::util::{
        ApproxTokenSeverity, approx_tool_output_token_severity, format_approx_token_count,
    };
    // Family-neutral, like the TUI badge: a result is shown before any model reads it.
    let tokens = estimate_text(TokenFamily::default(), output).max(usize::from(!output.is_empty()));
    let theme = Theme::global();
    let color = match approx_tool_output_token_severity(tokens) {
        ApproxTokenSeverity::Normal => theme.OK,
        ApproxTokenSeverity::Warning => theme.WARN,
        ApproxTokenSeverity::Danger => theme.ERROR,
    };
    // MCP results over budget arrive trimmed with a note giving the original
    // size. Show both, so the badge says what the server actually returned.
    if let Some(original) = trimmed_mcp_original_tokens(output) {
        return (
            format!(
                "~{} of {}",
                format_approx_token_count(tokens),
                format_approx_token_count(original).trim_end_matches(" tok")
            ),
            theme.WARN,
        );
    }
    (format!("~{}", format_approx_token_count(tokens)), color)
}

/// The original token count stated by the SDK's MCP trim note, if present.
pub(super) fn trimmed_mcp_original_tokens(output: &str) -> Option<usize> {
    const PREFIX: &str = "[MCP result trimmed from ";
    let rest = &output[output.rfind(PREFIX)? + PREFIX.len()..];
    rest.split_once(" to ")?.0.parse().ok()
}

/// The token badge custom tool cards show in their header, matching the
/// generic tool row.
pub(crate) fn tool_card_token_pill(output: &str) -> Option<gpui::Div> {
    if output.trim().is_empty() {
        return None;
    }
    let (label, color) = tool_output_token_badge(output);
    Some(
        div()
            .debug_selector(|| "tool-card-tokens".into())
            .flex_none()
            .px_2()
            .py(px(2.0))
            .rounded_full()
            .bg(Theme::global().INLINE_CODE_BG)
            .text_size(px(10.0))
            .line_height(px(14.0))
            .text_color(color)
            .child(label),
    )
}

/// `12500` -> `12.5k`, `1048576` -> `1.0m`; small counts stay exact.
pub(super) fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}m", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Collapse whitespace and clip to a readable length.
pub(super) fn condense(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let clipped: String = flat.chars().take(limit).collect();
    format!("{}…", clipped.trim_end())
}

/// Like `condense`, but keeps the end: while reasoning streams, the newest
/// words are the ones worth reading.
#[cfg(test)]
pub(super) fn condense_tail(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = flat.chars().count();
    if count <= limit {
        return flat;
    }
    let clipped: String = flat.chars().skip(count - limit).collect();
    format!("…{}", clipped.trim_start())
}

/// Drop ANSI escape sequences (colors, cursor moves) so terminal output
/// renders as text instead of garbage.
pub(super) fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            output.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ ... final byte in @-~
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-character sequences like ESC ( B.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    output
}

pub(super) fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub(super) fn tool_argument_visible(key: &str) -> bool {
    !matches!(
        key,
        "intent" | "label" | "id" | "task_id" | "call_id" | "session_id" | "request_id"
    ) && !key.ends_with("_id")
}
