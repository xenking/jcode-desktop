//! Model picker metadata comes from the daemon, including shared TUI preferences.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::provider_mark::provider_logo;
use jcode_sdk::{ModelRouteInfo, ModelUsage, compare_model_usage};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ModelDetails {
    pub model: String,
    pub recommended: bool,
    pub provider: String,
    pub api_method: String,
    pub usage: Option<ModelUsage>,
    pub speed_options: Vec<jcode_provider_core::SpeedOption>,
    /// What the saved login behind this route is called (`team · Max 20x`),
    /// from the catalog. `None` for routes that are not a saved OAuth login.
    pub account_name: Option<String>,
    /// The routing account (`provider:label`) behind the route, and its icon.
    pub account: Option<String>,
    pub icon: Option<String>,
}

impl ModelDetails {
    /// The icon that leads the route's account group: the daemon's resolved
    /// one, which it sends only where the provider has several accounts (or the
    /// user set one). `None` for routes that name no account.
    pub(super) fn account_icon(&self) -> Option<String> {
        self.account.as_ref()?;
        self.icon.clone()
    }

    /// The fields model search matches words against.
    pub(super) fn search_text(&self) -> super::model_search::RouteText<'_> {
        super::model_search::RouteText {
            model: &self.model,
            provider: &self.provider,
            api_method: &self.api_method,
        }
    }
}

/// Route identity: everything that decides its spec and recommendation.
/// Usage is deliberately absent, since usage ticks are what rebroadcast.
type RouteKey = (String, String, String, String);

thread_local! {
    static ROUTE_SPECS: std::cell::RefCell<HashMap<RouteKey, (String, bool)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Spec and recommendation, memoized. Every model usage tick resends the
/// full catalog, and re-deriving routing policy for each route was a steady
/// share of UI-thread time in live profiles. Bounded so catalog churn cannot
/// grow it without limit.
fn route_identity(route: &ModelRouteInfo) -> (String, bool) {
    const MAX_ENTRIES: usize = 8192;
    let key = (
        route.model.clone(),
        route.provider.clone(),
        route.api_method.clone(),
        route.detail.clone(),
    );
    ROUTE_SPECS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
        let value = (
            route_spec(route),
            jcode_provider_core::model_route_metadata_is_recommended(
                jcode_provider_core::explicit_model_provider_prefix(&route.model)
                    .map_or(route.model.as_str(), |(_, _, bare)| bare),
                &route.provider,
                &route.api_method,
                true,
            ),
        );
        if cache.len() >= MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(key, value.clone());
        value
    })
}

pub(super) fn from_routes(routes: &[ModelRouteInfo]) -> HashMap<String, ModelDetails> {
    let mut details = HashMap::<String, ModelDetails>::with_capacity(routes.len());
    for route in routes.iter().filter(|route| route.available) {
        let (spec, recommended) = route_identity(route);
        let candidate = ModelDetails {
            model: route.model.clone(),
            recommended,
            provider: route.provider.clone(),
            api_method: route.api_method.clone(),
            usage: route.usage.clone(),
            speed_options: route.speed_options.clone(),
            account_name: route_account_name(&route.api_method, &route.detail),
            account: route.account.clone(),
            icon: route.icon.clone(),
        };
        match details.get(&spec) {
            Some(existing)
                if !compare_model_usage(candidate.usage.as_ref(), existing.usage.as_ref())
                    .is_lt() => {}
            _ => {
                details.insert(spec, candidate);
            }
        }
    }
    details
}

/// The name of the saved login a route is pinned to: the user's name for it,
/// else its email, else the internal label. `None` when the route is not a
/// saved OAuth login. The detail only counts when it names the same login as
/// the api method, so a stray diagnostic never renames a group.
pub(crate) fn route_account_name(api_method: &str, detail: &str) -> Option<String> {
    let label = jcode_provider_core::oauth_account_label(api_method)?;
    let identity = jcode_provider_core::parse_route_account_identity(detail)
        .filter(|identity| identity.label == label);
    Some(match identity {
        Some(identity) => identity
            .display_name
            .or_else(|| identity.email())
            .unwrap_or(label)
            .to_owned(),
        None => label.to_owned(),
    })
}

/// Use the same routing policy as the daemon and TUI, never infer auth from a label.
pub(crate) fn route_spec(route: &ModelRouteInfo) -> String {
    use jcode_provider_core::{ModelRoute, RouteSelection, explicit_model_provider_prefix};
    let bare = explicit_model_provider_prefix(&route.model)
        .map_or(route.model.as_str(), |(_, _, bare)| bare);
    let mut selection = RouteSelection::from_model_route(&ModelRoute {
        reasoning_efforts: route.reasoning_efforts.clone(),
        model: bare.to_string(),
        provider: route.provider.clone(),
        api_method: route.api_method.clone(),
        available: route.available,
        detail: route.detail.clone(),
        cheapness: None,
        usage: route.usage.clone(),
        speed_options: Vec::new(),
        account: route.account.clone(),
        icon: route.icon.clone(),
    });
    let spec = selection.routed_model_spec();
    selection.model.clear();
    let prefix = selection.routed_model_spec();
    if !prefix.is_empty() && bare.starts_with(&prefix) {
        bare.to_string()
    } else {
        spec
    }
}

pub(super) fn current_spec<'a>(
    current: Option<&str>,
    models: &'a [String],
    details: &HashMap<String, ModelDetails>,
) -> Option<&'a str> {
    let current = current?;
    models
        .iter()
        .find(|model| model.as_str() == current)
        .or_else(|| {
            let mut matches = models
                .iter()
                .filter(|model| details.get(*model).is_some_and(|d| d.model == current));
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
        .map(String::as_str)
}

/// How one model id sorts inside its provider group.
///
/// Splits a catalog id such as `anthropic/claude-opus-5.5:batch` into the
/// vendor namespace, the family (`claude-opus`), the version (`[5, 5]`), and
/// whether it is a `~...-latest` alias or a `:tag` variant of a base model.
#[derive(Debug, Default, PartialEq, Eq)]
struct ModelKey {
    vendor: String,
    family: String,
    version: Vec<u64>,
    alias: bool,
    variant: String,
}

fn model_key(raw: &str) -> ModelKey {
    let bare = jcode_provider_core::explicit_model_provider_prefix(raw)
        .map_or(raw, |(_, _, bare)| bare)
        .to_ascii_lowercase();
    let (bare, mut alias) = match bare.strip_prefix('~') {
        Some(rest) => (rest.to_string(), true),
        None => (bare, false),
    };
    let (vendor, name) = bare.rsplit_once('/').unwrap_or(("", bare.as_str()));
    let vendor = vendor.trim_start_matches('~').to_string();
    let mut variant = String::new();
    let mut name = name.to_string();
    if let Some(index) = name.find('[') {
        variant = name.split_off(index);
    }
    if let Some((head, tag)) = name.split_once(':') {
        variant = format!("{tag}{variant}");
        name = head.to_string();
    }
    let mut family = Vec::new();
    let mut version = Vec::new();
    for token in name.split(['-', '_']).filter(|token| !token.is_empty()) {
        let numeric = token.strip_prefix('v').unwrap_or(token);
        if token == "latest" {
            alias = true;
        } else if !numeric.is_empty()
            && numeric.chars().all(|c| c.is_ascii_digit() || c == '.')
            && numeric.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            // Date stamps (`20251001`) tie-break nothing a reader can see.
            version.extend(
                numeric
                    .split('.')
                    .filter(|part| (1..6).contains(&part.len()))
                    .filter_map(|part| part.parse::<u64>().ok()),
            );
        } else {
            family.push(token);
        }
    }
    ModelKey {
        vendor,
        family: family.join("-"),
        version,
        alias,
        variant,
    }
}

impl ModelKey {
    /// A base release, not an alias or a `:batch`-style variant. Collapsed
    /// groups lead with these.
    fn primary(&self) -> bool {
        !self.alias && self.variant.is_empty()
    }
}

fn model_raw_name<'a>(spec: &'a str, details: &'a HashMap<String, ModelDetails>) -> &'a str {
    details
        .get(spec)
        .map_or(spec, |detail| detail.model.as_str())
}

/// Whether the collapsed picker should lead with this route.
pub(super) fn is_primary(spec: &str, details: &HashMap<String, ModelDetails>) -> bool {
    model_key(model_raw_name(spec, details)).primary()
}

type GroupSortKey = (bool, std::cmp::Reverse<(u64, u64)>, String, u8, String);

/// Usage weight of one route: tracked turns, then how recently it ran.
fn usage_weight(model: &str, details: &HashMap<String, ModelDetails>) -> (u64, u64) {
    details
        .get(model)
        .and_then(|detail| detail.usage.as_ref())
        .map_or((0, 0), |usage| {
            (usage.count, usage.last_used_unix_secs.unwrap_or(0))
        })
}

/// Provider groups sort by how much they are used (turns summed over the
/// group, then the most recent use), then by provider name, account before
/// key, then id. Unused groups keep the alphabetical order. Routes without
/// metadata trail everything.
fn group_sort_keys(
    models: &[String],
    details: &HashMap<String, ModelDetails>,
) -> HashMap<String, GroupSortKey> {
    let mut weights: HashMap<String, (u64, u64)> = HashMap::new();
    for model in models {
        let (count, last) = usage_weight(model, details);
        let entry = weights.entry(group_key(model, details)).or_default();
        entry.0 += count;
        entry.1 = entry.1.max(last);
    }
    models
        .iter()
        .map(|model| {
            let group = group_key(model, details);
            let weight = weights.get(&group).copied().unwrap_or_default();
            (model.clone(), group_sort_key(model, details, weight))
        })
        .collect()
}

fn group_sort_key(
    model: &str,
    details: &HashMap<String, ModelDetails>,
    weight: (u64, u64),
) -> GroupSortKey {
    if !details.contains_key(model) {
        return (
            true,
            std::cmp::Reverse((0, 0)),
            String::new(),
            0,
            String::new(),
        );
    }
    let parts = header_parts(model, details);
    let auth = match parts.auth {
        AuthKind::Account => 0,
        AuthKind::Subscription => 1,
        AuthKind::ApiKey => 2,
        AuthKind::CloudCredentials => 3,
        AuthKind::Other => 4,
    };
    (
        false,
        std::cmp::Reverse(weight),
        parts.provider.to_ascii_lowercase(),
        auth,
        group_key(model, details).to_ascii_lowercase(),
    )
}

/// Picker order: provider groups by usage, then inside a group the models
/// used most (turns, then recency) first. Unused models fall back to the
/// deterministic catalog order: vendor namespace, releases newest first (ties
/// by family name), then `~latest` aliases, with `:batch`-style variants right
/// behind their base model.
///
/// Every usage tick rebroadcasts the whole catalog to every panel, so this
/// runs often on the UI thread. Keys are derived once per route, not once per
/// comparison.
pub(super) fn rank(models: &mut Vec<String>, details: &HashMap<String, ModelDetails>) {
    let groups = group_sort_keys(models, details);
    let mut keyed: Vec<(GroupSortKey, (u64, u64), ModelKey, bool, String)> = std::mem::take(models)
        .into_iter()
        .map(|spec| {
            (
                groups[&spec].clone(),
                usage_weight(&spec, details),
                model_key(model_raw_name(&spec, details)),
                details.get(&spec).is_some_and(|detail| detail.recommended),
                spec,
            )
        })
        .collect();
    keyed.sort_by(
        |(a_group, a_use, a, a_rec, a_spec), (b_group, b_use, b, b_rec, b_spec)| {
            a_group
                .cmp(b_group)
                .then_with(|| b_use.0.cmp(&a_use.0))
                .then_with(|| a.vendor.cmp(&b.vendor))
                .then_with(|| a.alias.cmp(&b.alias))
                .then_with(|| b.version.cmp(&a.version))
                .then_with(|| a.family.cmp(&b.family))
                .then_with(|| a.variant.cmp(&b.variant))
                .then_with(|| b_rec.cmp(a_rec))
                .then_with(|| a_spec.cmp(b_spec))
        },
    );
    models.extend(keyed.into_iter().map(|(_, _, _, _, spec)| spec));
}

pub(super) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn relative_time(timestamp: u64, now: u64) -> String {
    let age = now.saturating_sub(timestamp);
    match age {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", age / 60),
        3600..86400 => format!("{}h ago", age / 3600),
        _ => format!("{}d ago", age / 86400),
    }
}

pub(super) fn usage_label(usage: Option<&ModelUsage>, now: u64) -> String {
    let Some(usage) = usage else {
        return "Usage history unavailable".into();
    };
    if usage.count > 0 {
        let count = usage.count;
        let unit = if count == 1 {
            "tracked turn"
        } else {
            "tracked turns"
        };
        let last = usage
            .last_used_unix_secs
            .map(|timestamp| format!("Last used {}", relative_time(timestamp, now)))
            .unwrap_or_else(|| "Last use unknown".into());
        return format!("{count} {unit} · {last}");
    }
    if usage.selection_count > 0 {
        let count = usage.selection_count;
        let unit = if count == 1 {
            "prior selection"
        } else {
            "prior selections"
        };
        let last = usage
            .last_selected_unix_secs
            .map(|timestamp| format!("Selected {}", relative_time(timestamp, now)))
            .unwrap_or_else(|| "Selection time unknown".into());
        return format!("{count} {unit} · {last} · No tracked turns");
    }
    "No recorded usage yet".into()
}

impl ModelDetails {
    /// Secondary line under the pretty title. The title already names the
    /// model and the group header names provider and auth, so only usage is
    /// new information. Search still matches the exact id.
    pub(super) fn label(&self, now: u64) -> String {
        usage_label(self.usage.as_ref(), now)
    }
}

/// How a route authenticates, phrased and iconed for the group header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthKind {
    /// Signed in with a provider account (ChatGPT, Claude, GitHub, Google).
    Account,
    ApiKey,
    Subscription,
    CloudCredentials,
    Other,
}

impl AuthKind {
    pub(super) fn of(api_method: &str) -> Self {
        use jcode_provider_core::ModelRouteApiMethod as M;
        match M::parse(api_method) {
            M::ClaudeOAuth
            | M::OpenAIOAuth
            | M::CodeAssistOAuth
            | M::Copilot
            | M::Cursor
            | M::AntigravityHttps
            | M::GrokBuild => Self::Account,
            M::AnthropicApiKey | M::OpenAIApiKey | M::OpenRouter | M::OpenAiCompatible { .. } => {
                Self::ApiKey
            }
            M::JcodeSubscription => Self::Subscription,
            M::Bedrock => Self::CloudCredentials,
            M::Other(method) => {
                let method = method.to_ascii_lowercase();
                if method.contains("oauth") {
                    Self::Account
                } else if method.contains("key") || method.ends_with("-api") {
                    Self::ApiKey
                } else {
                    Self::Other
                }
            }
            M::RemoteCatalog | M::Current => Self::Other,
        }
    }
}

/// Group header parts: provider brand, its logo id, and the auth method.
pub(super) struct HeaderParts {
    pub provider: String,
    pub logo: &'static str,
    pub auth: AuthKind,
}

/// The credential a route bills against, as `accounts` ids it (`claude`,
/// `openai-api`, `opencode-go`), plus the saved OAuth login it is pinned to.
/// The picker header shows that credential's quota.
pub(crate) fn route_credential(provider: &str, api_method: &str) -> (String, Option<String>) {
    use jcode_provider_core::ModelRouteApiMethod as M;
    let login = jcode_provider_core::oauth_account_label(api_method).map(str::to_owned);
    let id = match M::parse(api_method) {
        M::ClaudeOAuth => "claude".to_owned(),
        M::AnthropicApiKey => "anthropic-api".to_owned(),
        M::OpenAIOAuth => "openai".to_owned(),
        M::OpenAIApiKey => "openai-api".to_owned(),
        M::OpenRouter => "openrouter".to_owned(),
        M::Copilot => "copilot".to_owned(),
        M::Cursor => "cursor".to_owned(),
        M::JcodeSubscription => "jcode".to_owned(),
        M::OpenAiCompatible {
            profile_id: Some(profile),
        } => profile,
        _ => {
            let provider = provider
                .trim()
                .to_ascii_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
            let auth = (AuthKind::of(api_method) == AuthKind::ApiKey).then_some("api key");
            crate::accounts::credential_id(&provider, auth)
        }
    };
    (id, login)
}

/// One quota window for a picker header bar.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuotaWindow {
    /// Short window name: `5h`, `7d`, or the provider's first word.
    pub name: String,
    /// Percent consumed, 0 to 100.
    pub used: f32,
    pub reset_in: Option<String>,
}

/// What a provider header shows beside its name: one bar per quota window,
/// banked resets, or a money figure when the credential has no quota API.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RouteQuota {
    pub windows: Vec<QuotaWindow>,
    pub banked_resets: u64,
    /// `$12.34` balance or spend, only when no window is reported.
    pub money: Option<String>,
}

impl RouteQuota {
    pub(crate) fn is_empty(&self) -> bool {
        self.windows.is_empty() && self.banked_resets == 0 && self.money.is_none()
    }
}

fn short_window_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.starts_with("5-hour") || lower.starts_with("5 hour") {
        "5h".into()
    } else if lower.starts_with("7-day") || lower.starts_with("7 day") || lower.contains("week") {
        "7d".into()
    } else {
        name.split_whitespace().next().unwrap_or(name).to_owned()
    }
}

fn first_usd(text: &str) -> Option<String> {
    let start = text.find('$')?;
    let amount: String = text[start + 1..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = amount.parse().ok()?;
    Some(format!("${value:.2}"))
}

/// The quota of the credential `provider`/`api_method` bills against. A route
/// pinned to one OAuth login shows that login's report, never another's.
pub(crate) fn route_quota(
    accounts: &[crate::accounts::Account],
    provider: &str,
    api_method: &str,
) -> RouteQuota {
    let (id, login) = route_credential(provider, api_method);
    let Some(account) = accounts
        .iter()
        .find(|account| account.id == id && account.available())
    else {
        return RouteQuota::default();
    };
    let report = match &login {
        Some(login) => account
            .usage_reports
            .iter()
            .find(|report| report.account_label.as_deref() == Some(login.as_str())),
        None => account.active_report(),
    };
    let limits = match (&login, report) {
        (_, Some(report)) => report.limits.as_slice(),
        (None, None) => account.active_limits().unwrap_or_default(),
        (Some(_), None) => &[],
    };
    let windows: Vec<QuotaWindow> = limits
        .iter()
        .filter(|limit| limit.usage_percent.is_finite())
        .map(|limit| QuotaWindow {
            name: short_window_name(&limit.name),
            used: limit.usage_percent.clamp(0., 100.),
            reset_in: limit.reset_in.clone(),
        })
        .collect();
    let banked_resets = report
        .and_then(|report| report.banked_reset.as_ref())
        .map_or(0, |reset| reset.available_count);
    let money = if windows.is_empty() {
        let info = report.map_or(&[][..], |report| report.extra_info.as_slice());
        ["Balance", "Local spend (this machine)", "Today"]
            .iter()
            .find_map(|key| {
                info.iter()
                    .find(|(name, _)| name == key)
                    .and_then(|(_, value)| first_usd(value))
            })
    } else {
        None
    };
    RouteQuota {
        windows,
        banked_resets,
        money,
    }
}

pub(super) fn header_parts(model: &str, details: &HashMap<String, ModelDetails>) -> HeaderParts {
    let Some(detail) = details.get(model) else {
        return HeaderParts {
            provider: "Other models".into(),
            logo: "",
            auth: AuthKind::Other,
        };
    };
    let provider = if detail.provider.is_empty() {
        crate::accounts::pretty_provider_name(&detail.api_method)
    } else {
        crate::accounts::pretty_provider_name(&detail.provider)
    };
    HeaderParts {
        logo: provider_logo(&detail.provider, &detail.api_method),
        provider,
        auth: AuthKind::of(&detail.api_method),
    }
}

/// Logo id for the model's own family, so an OpenAI model reads as OpenAI
/// whether it is served directly or through OpenRouter. Unknown families
/// fall back to the serving provider's logo.
pub(super) fn model_logo(model: &str, details: &HashMap<String, ModelDetails>) -> &'static str {
    let detail = details.get(model);
    let raw = detail.map_or(model, |detail| detail.model.as_str());
    let raw = jcode_provider_core::explicit_model_provider_prefix(raw)
        .map_or(raw, |(_, _, bare)| bare)
        .to_ascii_lowercase();
    let (vendor, name) = raw.rsplit_once('/').unwrap_or(("", raw.as_str()));
    let vendor = vendor.rsplit('/').next().unwrap_or(vendor);
    let by_vendor = match vendor {
        "anthropic" => "anthropic-api",
        "openai" => "openai",
        "google" => "gemini",
        "x-ai" | "xai" => "xai",
        "mistralai" | "mistral" => "mistral",
        "deepseek" | "deepseek-ai" => "deepseek",
        "moonshotai" => "kimi",
        "qwen" => "alibaba-coding-plan",
        "z-ai" | "zai" | "zhipuai" => "zai",
        "minimax" => "minimax",
        _ => "",
    };
    if !by_vendor.is_empty() {
        return by_vendor;
    }
    // Bedrock-style `anthropic.claude-...` ids name the family after a dot.
    let name = name.split_once('.').map_or(name, |(head, rest)| {
        if head.chars().all(|c| c.is_ascii_alphabetic()) {
            rest
        } else {
            name
        }
    });
    for (prefix, logo) in [
        ("claude", "anthropic-api"),
        ("gpt", "openai"),
        ("codex", "openai"),
        ("o1", "openai"),
        ("o3", "openai"),
        ("o4", "openai"),
        ("gemini", "gemini"),
        ("gemma", "gemini"),
        ("grok", "xai"),
        ("mistral", "mistral"),
        ("codestral", "mistral"),
        ("devstral", "mistral"),
        ("deepseek", "deepseek"),
        ("kimi", "kimi"),
        ("qwen", "alibaba-coding-plan"),
        ("glm", "zai"),
        ("minimax", "minimax"),
    ] {
        if name.starts_with(prefix) {
            return logo;
        }
    }
    detail.map_or("", |detail| {
        provider_logo(&detail.provider, &detail.api_method)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(count: u64, last: Option<u64>, selections: u64) -> ModelUsage {
        ModelUsage {
            count,
            last_used_unix_secs: last,
            tracking_started_unix_secs: Some(10),
            selection_count: selections,
            last_selected_unix_secs: Some(20),
        }
    }

    fn route(model: &str, method: &str, count: u64) -> ModelRouteInfo {
        ModelRouteInfo {
            account: None,
            icon: None,
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: model.into(),
            provider: "OpenAI".into(),
            api_method: method.into(),
            available: true,
            detail: String::new(),
            usage: Some(usage(count, Some(count), 0)),
        }
    }

    #[test]
    fn three_saved_logins_of_one_provider_read_as_three_named_groups() {
        let login = |label: &str, detail: &str| {
            let mut route = route("claude-opus-5-5", &format!("claude-oauth@{label}"), 1);
            route.provider = "Anthropic".into();
            route.detail = detail.into();
            route
        };
        let routes = [
            login(
                "claude-otter",
                "account claude-otter [team · Max 20x] · t***m@example.com",
            ),
            login("claude-fox", "account claude-fox [m***x@example.com · Max]"),
            // An older daemon sends no name: the email still tells it apart.
            login("claude-panda", "account claude-panda · e***t@example.com"),
        ];
        let details = from_routes(&routes);
        assert_eq!(details.len(), 3);
        let mut labels: Vec<String> = details
            .keys()
            .map(|spec| group_label(spec, &details))
            .collect();
        labels.sort();
        assert_eq!(
            labels,
            [
                "Anthropic · e***t@example.com",
                "Anthropic · m***x@example.com · Max",
                "Anthropic · team · Max 20x",
            ]
        );
        // A diagnostic naming another login never renames a group.
        assert_eq!(
            route_account_name("claude-oauth@claude-fox", "account claude-otter [team]"),
            Some("claude-fox".into())
        );
        assert_eq!(route_account_name("claude-oauth", "account x [y]"), None);
    }

    #[test]
    fn groups_keep_auth_routes_distinct_and_canonicalize_aliases() {
        let routes = [
            route("atlas", "openai-oauth", 2),
            route("atlas", "openai-api-key", 1),
            route("openai-oauth:atlas", "openai-oauth", 0),
        ];
        let details = from_routes(&routes);
        assert_eq!(details.len(), 2);
        assert!(details.contains_key("openai-oauth:atlas"));
        assert!(details.contains_key("openai-api:atlas"));
        assert_eq!(
            route_spec(&route("openai:atlas", "openai-api-key", 0)),
            "openai-api:atlas"
        );
        let rows = grouped_rows(
            &details.keys().cloned().collect::<Vec<_>>(),
            &details,
            None,
            &Default::default(),
            "",
        );
        assert_eq!(rows.iter().filter(|r| r.header.is_some()).count(), 2);
        assert_eq!(rows[0].value, "/model openai-oauth:atlas");
        assert_eq!(rows[1].value, "/model openai-api:atlas");
        assert_eq!(
            route_spec(&route("bedrock:anthropic.claude-v1:0", "bedrock", 0)),
            "bedrock:anthropic.claude-v1:0"
        );
    }

    #[test]
    fn groups_show_three_distinct_ranked_models_and_search_hidden_choices() {
        let routes: Vec<_> = (1..=6)
            .map(|n| route(&format!("atlas-{n}"), "openai-oauth", n))
            .collect();
        let details = from_routes(&routes);
        let mut models: Vec<_> = details.keys().cloned().collect();
        models.push("openai-oauth:atlas-6".into());
        let closed = grouped_rows(&models, &details, Some("atlas-1"), &Default::default(), "");
        assert_eq!(
            closed.iter().map(|r| r.value.as_str()).collect::<Vec<_>>(),
            [
                "/model openai-oauth:atlas-1",
                "/model openai-oauth:atlas-6",
                "/model openai-oauth:atlas-5",
                "Show 3 more models"
            ]
        );
        assert!(closed[0].header.is_some());
        let key = closed[3].toggle.clone().unwrap();
        let expanded = [key].into_iter().collect();
        let open = grouped_rows(&models, &details, Some("atlas-1"), &expanded, "");
        assert_eq!(open.len(), 7);
        assert_eq!(open[6].value, "Show fewer models");
        let search = grouped_rows(&models, &details, None, &Default::default(), "ATLAS-2");
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].value, "/model openai-oauth:atlas-2");
        assert!(search[0].toggle.is_none());
        assert_eq!(
            grouped_rows(&models, &details, None, &Default::default(), "oauth").len(),
            6
        );
        assert!(grouped_rows(&models, &details, None, &expanded, "missing").is_empty());
    }

    #[test]
    fn recommendation_breaks_usage_ties_and_unavailable_routes_are_excluded() {
        let mut unavailable = route("unavailable", "openai-oauth", 100);
        unavailable.available = false;
        let recommended = jcode_provider_core::DEFAULT_OPENAI_MODEL;
        let details = from_routes(&[
            route("a-unknown", "openai-oauth", 0),
            route(recommended, "openai-oauth", 0),
            unavailable,
        ]);
        let mut models: Vec<_> = details.keys().cloned().collect();
        rank(&mut models, &details);
        assert_eq!(
            models,
            [
                format!("openai-oauth:{recommended}"),
                "openai-oauth:a-unknown".to_string()
            ]
        );
    }

    #[test]
    fn metadata_does_not_claim_unknown_or_untracked_models_were_never_used() {
        assert_eq!(usage_label(None, 100), "Usage history unavailable");
        assert_eq!(
            usage_label(Some(&usage(0, None, 0)), 100),
            "No recorded usage yet"
        );
        assert_eq!(
            usage_label(Some(&usage(0, None, 4)), 100),
            "4 prior selections · Selected 1m ago · No tracked turns"
        );
        assert_eq!(
            usage_label(Some(&usage(1, Some(90), 4)), 100),
            "1 tracked turn · Last used just now"
        );
        assert_eq!(
            usage_label(Some(&usage(12, Some(100), 4)), 7300),
            "12 tracked turns · Last used 2h ago"
        );
        assert_eq!(relative_time(100, 864100), "10d ago");
        assert_eq!(relative_time(200, 100), "just now");
    }

    #[test]
    fn row_logo_names_the_model_maker_and_headers_name_route_and_auth() {
        let mut routed = route("openai/gpt-6-astra", "openrouter", 0);
        routed.provider = "OpenRouter".into();
        let details = from_routes(&[
            route("gpt-6-astra", "openai-api-key", 3),
            route("gpt-6-astra", "openai-oauth", 1),
            routed,
        ]);
        assert_eq!(details.len(), 3);
        for (spec, detail) in &details {
            assert_eq!(model_logo(spec, &details), "openai", "{spec}");
            let parts = header_parts(spec, &details);
            let expected = match detail.api_method.as_str() {
                "openrouter" => ("OpenRouter", "openrouter", AuthKind::ApiKey),
                "openai-oauth" => ("OpenAI", "openai", AuthKind::Account),
                _ => ("OpenAI", "openai", AuthKind::ApiKey),
            };
            assert_eq!((parts.provider.as_str(), parts.logo, parts.auth), expected);
        }
        assert_eq!(AuthKind::of("jcode-subscription"), AuthKind::Subscription);
        assert_eq!(AuthKind::of("bedrock"), AuthKind::CloudCredentials);
        assert_eq!(AuthKind::of("claude-oauth"), AuthKind::Account);
        let mut bedrock = route("anthropic.claude-v1:0", "bedrock", 0);
        bedrock.provider = "Bedrock".into();
        let details = from_routes(&[bedrock]);
        let spec = details.keys().next().unwrap();
        assert_eq!(model_logo(spec, &details), "anthropic-api");
        // Unknown families fall back to the serving provider.
        let mut private = route("house-model", "openrouter", 0);
        private.provider = "OpenRouter".into();
        let details = from_routes(&[private]);
        let spec = details.keys().next().unwrap();
        assert_eq!(model_logo(spec, &details), "openrouter");
    }

    fn account(id: &str, reports: Vec<crate::accounts::UsageReport>) -> crate::accounts::Account {
        crate::accounts::Account {
            id: id.into(),
            display_name: id.into(),
            status: "available".into(),
            auth_kind: "OAuth".into(),
            method: String::new(),
            limits: Vec::new(),
            usage_reports: reports,
        }
    }

    fn report(
        name: &str,
        label: Option<&str>,
        limits: &[(&str, f32, &str)],
        info: &[(&str, &str)],
    ) -> crate::accounts::UsageReport {
        crate::accounts::UsageReport {
            identity: None,
            credits: None,
            provider_name: name.into(),
            account_label: label.map(str::to_owned),
            limits: limits
                .iter()
                .map(|(name, used, reset)| crate::accounts::UsageLimit {
                    name: (*name).into(),
                    usage_percent: *used,
                    reset_in: Some((*reset).into()),
                })
                .collect(),
            extra_info: info
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            banked_reset: None,
        }
    }

    #[test]
    fn routes_map_to_the_credential_they_bill() {
        for (provider, method, id) in [
            ("Anthropic", "claude-oauth", "claude"),
            ("Anthropic", "oauth", "claude"),
            ("Anthropic", "anthropic-api-key", "anthropic-api"),
            ("Anthropic", "api key", "anthropic-api"),
            ("OpenAI", "openai-oauth", "openai"),
            ("OpenAI", "openai-api-key", "openai-api"),
            ("OpenRouter", "openrouter", "openrouter"),
            ("OpenCode Go", "api key", "opencode-go"),
        ] {
            assert_eq!(
                route_credential(provider, method).0,
                id,
                "{provider} {method}"
            );
        }
        assert_eq!(
            route_credential("OpenAI", "openai-oauth@openai-otter"),
            ("openai".into(), Some("openai-otter".into()))
        );
    }

    #[test]
    fn header_quota_comes_from_the_routes_own_login() {
        let accounts = [
            account(
                "openai",
                vec![
                    report(
                        "OpenAI (ChatGPT) (a@x) ✦",
                        Some("openai-fox"),
                        &[("5-hour window", 20., "2h")],
                        &[],
                    ),
                    report(
                        "OpenAI (ChatGPT) (b@x)",
                        Some("openai-otter"),
                        &[("7-day window", 100., "2d 20h")],
                        &[],
                    ),
                ],
            ),
            account(
                "opencode-go",
                vec![report(
                    "OpenCode Go (API key)",
                    None,
                    &[],
                    &[("Today", "1.2 Mil in · $3.456 est.")],
                )],
            ),
        ];
        let active = route_quota(&accounts, "OpenAI", "openai-oauth");
        assert_eq!(
            active.windows,
            [QuotaWindow {
                name: "5h".into(),
                used: 20.,
                reset_in: Some("2h".into())
            }]
        );
        let pinned = route_quota(&accounts, "OpenAI", "openai-oauth@openai-otter");
        assert_eq!(pinned.windows[0].name, "7d");
        assert_eq!(pinned.windows[0].used, 100.);
        let key = route_quota(&accounts, "OpenCode Go", "api key");
        assert!(key.windows.is_empty());
        assert_eq!(key.money.as_deref(), Some("$3.46"));
        assert!(route_quota(&accounts, "Anthropic", "claude-oauth").is_empty());
    }

    #[test]
    fn detail_line_is_usage_only() {
        let detail =
            &from_routes(&[route("gpt-6-astra", "openai-api-key", 0)])["openai-api:gpt-6-astra"];
        assert_eq!(detail.label(100), "No recorded usage yet");
    }

    #[test]
    fn rank_puts_used_models_first_and_retains_every_model() {
        let mut models = vec![
            "unknown-z".into(),
            "frequent".into(),
            "unknown-a".into(),
            "recent".into(),
            "legacy".into(),
        ];
        let details = [
            ("frequent", usage(12, Some(100), 0)),
            ("recent", usage(12, Some(200), 0)),
            ("legacy", usage(0, None, 9)),
        ]
        .into_iter()
        .map(|(model, usage)| {
            (
                model.into(),
                ModelDetails {
                    model: model.into(),
                    recommended: false,
                    provider: String::new(),
                    api_method: String::new(),
                    usage: Some(usage),
                    speed_options: Vec::new(),
                    account_name: None,
                    account: None,
                    icon: None,
                },
            )
        })
        .collect();
        rank(&mut models, &details);
        assert_eq!(
            models,
            ["frequent", "recent", "legacy", "unknown-a", "unknown-z"]
        );
    }
}

/// A route header belongs to its first selectable row, not the keyboard index space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroupRow {
    pub value: String,
    pub header: Option<String>,
    pub toggle: Option<String>,
}

pub(super) fn group_key(model: &str, details: &HashMap<String, ModelDetails>) -> String {
    details
        .get(model)
        .map(|d| format!("{} · {}", d.provider, d.api_method))
        .unwrap_or_else(|| "Other models".into())
}

fn group_label(model: &str, details: &HashMap<String, ModelDetails>) -> String {
    let parts = header_parts(model, details);
    if details.get(model).is_none() {
        return parts.provider;
    }
    // The header shows the credential's quota, not a word for how it signs
    // in. Only a key route says so, to tell it apart from the same
    // provider's account route. A saved login says which one it is, so three
    // Anthropic accounts never read as three identical groups.
    match parts.auth {
        AuthKind::ApiKey => format!("{} · API key", parts.provider),
        AuthKind::CloudCredentials => format!("{} · Cloud", parts.provider),
        _ => match details.get(model).and_then(|d| d.account_name.as_deref()) {
            Some(name) => format!("{} · {name}", parts.provider),
            None => parts.provider,
        },
    }
}

/// Friendly picker title for a route spec (`claude-oauth:claude-opus-4-8`
/// reads `Claude Opus 4.8`). The exact id stays visible in the detail line.
pub(super) fn pretty_title(model: &str, details: &HashMap<String, ModelDetails>) -> String {
    let raw = details
        .get(model)
        .map_or(model, |detail| detail.model.as_str());
    let pretty = jcode_provider_core::model_names::pretty_picker_model_name(raw);
    if pretty.is_empty() {
        raw.to_string()
    } else {
        pretty
    }
}

/// The three rows a closed group shows, still in ranked order: the current
/// model, then base releases, and only then aliases and variants.
fn collapsed_members(
    members: Vec<String>,
    current: Option<&str>,
    details: &HashMap<String, ModelDetails>,
) -> Vec<String> {
    let mut chosen: Vec<bool> = members
        .iter()
        .map(|model| current == Some(model.as_str()))
        .collect();
    for primary_only in [true, false] {
        for (index, model) in members.iter().enumerate() {
            if chosen.iter().filter(|chosen| **chosen).count() >= 3 {
                break;
            }
            if !chosen[index] && (!primary_only || is_primary(model, details)) {
                chosen[index] = true;
            }
        }
    }
    members
        .into_iter()
        .zip(chosen)
        .filter_map(|(model, chosen)| chosen.then_some(model))
        .collect()
}

pub(super) fn grouped_rows(
    models: &[String],
    details: &HashMap<String, ModelDetails>,
    current: Option<&str>,
    expanded: &std::collections::HashSet<String>,
    query: &str,
) -> Vec<GroupRow> {
    let mut ranked = models.to_vec();
    rank(&mut ranked, details);
    let current = current_spec(current, &ranked, details).map(str::to_string);
    ranked.sort_by_key(|model| current.as_deref() != Some(model.as_str()));
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut group_indices = HashMap::new();
    let mut seen = std::collections::HashSet::with_capacity(ranked.len());
    for model in ranked {
        // Preserve ranked first occurrence and group order without repeatedly
        // scanning every previous group or every member of a large group.
        if !seen.insert(model.clone()) {
            continue;
        }
        let key = group_key(&model, details);
        let index = *group_indices.entry(key.clone()).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(model);
    }
    let query = super::model_search::ModelQuery::new(query);
    let searching = !query.is_empty();
    if searching {
        // Rank every route, then keep provider grouping but order groups by
        // their best hit so Enter always selects the strongest match.
        let ranked = super::model_search::rank_matches(
            groups.iter().flat_map(|(_, members)| members.iter()),
            |model| details.get(model).map(ModelDetails::search_text),
            |model| {
                format!(
                    "{} {}",
                    group_label(model, details),
                    group_key(model, details)
                )
            },
            &query,
        );
        let scores: HashMap<&String, (usize, i32)> = ranked
            .iter()
            .enumerate()
            .map(|(order, (model, score))| (*model, (order, *score)))
            .collect();
        let mut matched: Vec<(usize, Vec<String>)> = groups
            .iter()
            .filter_map(|(_, members)| {
                let mut hits: Vec<_> = members
                    .iter()
                    .filter_map(|model| scores.get(model).map(|(order, _)| (*order, model.clone())))
                    .collect();
                hits.sort_by_key(|(order, _)| *order);
                let best = hits.first()?.0;
                Some((best, hits.into_iter().map(|(_, model)| model).collect()))
            })
            .collect();
        matched.sort_by_key(|(best, _)| *best);
        let mut rows = Vec::new();
        for (_, members) in matched {
            for (index, model) in members.into_iter().enumerate() {
                rows.push(GroupRow {
                    header: (index == 0).then(|| group_label(&model, details)),
                    value: format!("/model {model}"),
                    toggle: None,
                });
            }
        }
        return rows;
    }
    let mut rows = Vec::new();
    for (key, matching) in groups {
        let count = matching.len();
        let open = expanded.contains(&key);
        let visible = if open {
            matching
        } else {
            collapsed_members(matching, current.as_deref(), details)
        };
        for (index, model) in visible.into_iter().enumerate() {
            rows.push(GroupRow {
                header: (index == 0).then(|| group_label(&model, details)),
                value: format!("/model {model}"),
                toggle: None,
            });
        }
        if count > 3 {
            rows.push(GroupRow {
                value: if open {
                    "Show fewer models".into()
                } else {
                    format!("Show {} more models", count - 3)
                },
                header: None,
                toggle: Some(key),
            });
        }
    }
    rows
}

#[cfg(test)]
mod grouping_performance_tests {
    use super::*;
    use std::collections::HashSet;

    fn fixture(count: usize, group_count: usize) -> (Vec<String>, HashMap<String, ModelDetails>) {
        let models: Vec<_> = (0..count)
            .map(|i| format!("provider:model-{i:04}"))
            .collect();
        let details = models
            .iter()
            .enumerate()
            .map(|(i, model)| {
                (
                    model.clone(),
                    ModelDetails {
                        model: format!("model-{i:04}"),
                        recommended: i % 7 == 0,
                        provider: format!("Provider {}", i % group_count),
                        api_method: "openai-oauth".into(),
                        usage: None,
                        speed_options: Vec::new(),
                        account_name: None,
                        account: None,
                        icon: None,
                    },
                )
            })
            .collect();
        (models, details)
    }

    #[test]
    fn indexed_grouping_matches_legacy_order_filtering_and_deduplication() {
        for group_count in [1, 8, 200] {
            let (mut models, details) = fixture(200, group_count);
            // Deliberately repeated and missing-metadata entries exercise stable
            // deduplication and the fallback group, not just unique route maps.
            models.extend(models.clone());
            models.extend(["unknown-model".into(), "unknown-model".into()]);
            let expanded: HashSet<_> = models.iter().map(|m| group_key(m, &details)).collect();
            for expansion in [&HashSet::new(), &expanded] {
                // Search is ranked by `model_search` and intentionally differs
                // from the legacy substring filter; browsing must not.
                for query in [""] {
                    for current in [None, Some("provider:model-0199"), Some("model-0005")] {
                        assert_eq!(
                            grouped_rows(&models, &details, current, expansion, query),
                            legacy_grouped_rows(&models, &details, current, expansion, query),
                            "groups={group_count}, query={query}, current={current:?}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "opt-in CPU microbenchmark, run with --ignored --nocapture"]
    fn grouped_rows_profile() {
        use std::hint::black_box;
        use std::time::Instant;
        type Grouping = fn(
            &[String],
            &HashMap<String, ModelDetails>,
            Option<&str>,
            &HashSet<String>,
            &str,
        ) -> Vec<GroupRow>;
        for count in [40, 200, 1000] {
            for group_count in [1, 8] {
                let (models, details) = fixture(count, group_count);
                let closed = HashSet::new();
                let expanded: HashSet<_> = models.iter().map(|m| group_key(m, &details)).collect();
                for (scenario, expansion, query) in [
                    ("collapsed", &closed, ""),
                    ("expanded", &expanded, ""),
                    ("broad-search", &closed, "model"),
                    ("narrow-search", &closed, "model-001"),
                    ("no-match", &closed, "missing"),
                ] {
                    let current = Some(models[count - 1].as_str());
                    if query.is_empty() {
                        assert_eq!(
                            grouped_rows(&models, &details, current, expansion, query),
                            legacy_grouped_rows(&models, &details, current, expansion, query)
                        );
                    }
                    for (implementation, group) in [
                        ("legacy", legacy_grouped_rows as Grouping),
                        ("indexed", grouped_rows as Grouping),
                    ] {
                        for _ in 0..20 {
                            black_box(group(&models, &details, current, expansion, query));
                        }
                        let mut micros = Vec::with_capacity(200);
                        for _ in 0..200 {
                            let started = Instant::now();
                            black_box(group(
                                black_box(&models),
                                &details,
                                current,
                                expansion,
                                query,
                            ));
                            micros.push(started.elapsed().as_secs_f64() * 1_000_000.);
                        }
                        micros.sort_by(f64::total_cmp);
                        eprintln!(
                            "grouped_rows {implementation} n={count} groups={group_count} {scenario}: p50={:.1}us p95={:.1}us max={:.1}us",
                            micros[100], micros[189], micros[199]
                        );
                    }
                }
            }
        }
    }

    // Frozen pre-indexing implementation: a same-binary reference for both
    // correctness and performance, not a second production code path.
    fn legacy_grouped_rows(
        models: &[String],
        details: &HashMap<String, ModelDetails>,
        current: Option<&str>,
        expanded: &std::collections::HashSet<String>,
        query: &str,
    ) -> Vec<GroupRow> {
        let mut ranked = models.to_vec();
        rank(&mut ranked, details);
        let current = current_spec(current, &ranked, details).map(str::to_string);
        ranked.sort_by_key(|model| current.as_deref() != Some(model.as_str()));
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for model in ranked {
            let key = group_key(&model, details);
            let index = groups
                .iter()
                .position(|(group, _)| group == &key)
                .unwrap_or_else(|| {
                    groups.push((key.clone(), Vec::new()));
                    groups.len() - 1
                });
            let members = &mut groups[index].1;
            if !members.contains(&model) {
                members.push(model);
            }
        }
        let query = query.trim().to_ascii_lowercase();
        let mut rows = Vec::new();
        for (key, members) in groups {
            let searching = !query.is_empty();
            let matching: Vec<_> = members
                .into_iter()
                .filter(|model| {
                    !searching
                        || model.to_ascii_lowercase().contains(&query)
                        || details.get(model).is_some_and(|detail| {
                            detail.model.to_ascii_lowercase().contains(&query)
                        })
                        || group_label(model, details)
                            .to_ascii_lowercase()
                            .contains(&query)
                        || key.to_ascii_lowercase().contains(&query)
                })
                .collect();
            let count = matching.len();
            let open = expanded.contains(&key);
            for (index, model) in matching
                .into_iter()
                .take(if searching || open { usize::MAX } else { 3 })
                .enumerate()
            {
                rows.push(GroupRow {
                    header: (index == 0).then(|| group_label(&model, details)),
                    value: format!("/model {model}"),
                    toggle: None,
                });
            }
            if !searching && count > 3 {
                rows.push(GroupRow {
                    value: if open {
                        "Show fewer models".into()
                    } else {
                        format!("Show {} more models", count - 3)
                    },
                    header: None,
                    toggle: Some(key),
                });
            }
        }
        rows
    }
}
