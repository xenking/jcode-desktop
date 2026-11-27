//! Account rows for the Accounts picker: one pill per login, with its name,
//! limits and recorded usage, grouped by auto-switch membership. Rows drag
//! between the Auto-switch and Manual groups and reorder within Auto-switch.
//! Only labels, masked emails and usage figures are shown, never credentials.
use super::connection::{ConnectionStatus, ConnectionStatuses, status_for};
use super::*;
use crate::accounts::{Account, BankedReset, ResetProvider, UsageLimit, UsageReport};
use jcode_base::auth::account_pool::{AccountPool, OAuthLogin, account_key};

/// Providers that store several named OAuth logins.
const MULTI_ACCOUNT: [&str; 2] = ["openai", "claude"];
/// Asking for an unknown label makes the CLI allocate the next free one.
pub(super) const NEW_ACCOUNT_LABEL: &str = "new";

#[derive(Default)]
pub(super) struct AccountsData {
    pub accounts: Option<Vec<Account>>,
    pub logins: Vec<OAuthLogin>,
    pub pool: AccountPool,
    /// Saved `default_provider`, the source of truth for the Default chip.
    pub default_route: Option<String>,
    /// The account that route resolves to, computed with the rows.
    pub default_key: Option<String>,
}

impl AccountsData {
    /// Resolve which connected account the saved default route selects.
    pub(super) fn resolve_default(&mut self, providers: &[LoginProvider]) {
        let Some(route) = self.default_route.clone() else {
            self.default_key = None;
            return;
        };
        let keys: Vec<String> = build_rows(providers, None, false, &|_| false, self)
            .into_iter()
            .filter(AccountRow::connected)
            .map(|row| row.key)
            .collect();
        let logins = &self.logins;
        let active = |provider: &str| {
            logins
                .iter()
                .find(|login| login.provider == provider && login.active)
                .or_else(|| logins.iter().find(|login| login.provider == provider))
                .map(|login| login.label.clone())
        };
        self.default_key = jcode_base::auth::account_pool::account_for_route(
            &route,
            keys.iter().map(String::as_str),
            active,
        )
        .map(str::to_owned);
    }
}

#[derive(Clone, Debug)]
pub(super) struct AccountRow {
    pub key: String,
    pub provider: LoginProvider,
    pub label: Option<String>,
    pub email: Option<String>,
    /// What the login is called for people (`team · Max 20x`), from the usage
    /// report. The internal label only shows when nothing better is known.
    pub name: Option<String>,
    pub active: bool,
    pub status: ConnectionStatus,
    pub limits: Vec<UsageLimit>,
    pub usage: Option<String>,
    pub plan: Option<String>,
    /// Banked usage resets this login holds (OpenAI) or a Claude session reset.
    pub banked: Option<BankedReset>,
    /// The auth report says a credential is stored, whatever its health.
    pub configured: bool,
    /// Why the last usage probe failed, for a rejected credential.
    pub error: Option<String>,
    /// First row of its provider keeps the stable `login-provider-{id}` selector.
    pub first_of_provider: bool,
}

impl AccountRow {
    pub(super) fn connected(&self) -> bool {
        self.label.is_some()
            || self.configured
            || matches!(
                self.status,
                ConnectionStatus::Connected
                    | ConnectionStatus::Testing
                    | ConnectionStatus::Unverified
                    | ConnectionStatus::Expired
                    | ConnectionStatus::Failed
            )
    }

    /// Subscriptions (and the Jcode subscription) rotate by default. Metered
    /// API keys never do silently. The policy lives in the SDK.
    fn default_pooled(&self) -> bool {
        jcode_base::auth::account_pool::default_member(
            &self.key,
            self.provider.method == LoginMethod::ApiKey,
        )
    }

    pub(super) fn subtitle(&self) -> Option<String> {
        let who = self.name.as_deref().or(self.label.as_deref());
        // A name that already carries the email would repeat it.
        let email = self.email.as_deref().filter(|email| {
            self.name
                .as_deref()
                .is_none_or(|name| !name.contains(*email))
        });
        let parts: Vec<&str> = [who, email].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

#[derive(Clone)]
pub(super) struct DraggedAccount {
    pub(super) key: String,
    pub(super) title: String,
}

pub(super) struct Groups {
    pub pooled: Vec<AccountRow>,
    pub manual: Vec<AccountRow>,
    pub available: Vec<AccountRow>,
    /// Connected multi-account providers that can take another login.
    pub addable: Vec<LoginProvider>,
}

fn report_for<'a>(
    account: &'a Account,
    label: Option<&str>,
    logins: usize,
) -> Option<&'a UsageReport> {
    let reports = &account.usage_reports;
    if let Some(label) = label {
        let exact = reports.iter().find(|report| {
            report.account_label.as_deref() == Some(label) || report.provider_name.contains(label)
        });
        if exact.is_some() {
            return exact;
        }
        return (logins == 1 && reports.len() == 1).then(|| &reports[0]);
    }
    (reports.len() == 1).then(|| &reports[0])
}

/// First `$1.23` amount in a usage line, trimmed to cents.
fn dollars(text: &str) -> Option<String> {
    let start = text.find('$')?;
    let amount: String = text[start + 1..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = amount.parse().ok()?;
    Some(format!("${value:.2}"))
}

fn usage_summary(report: &UsageReport) -> (Option<String>, Option<String>) {
    let value = |key: &str| {
        report
            .extra_info
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let plan = value("Plan").map(str::to_owned);
    // A balance the provider stated leads. An unknown one is not a figure.
    let balance = report
        .credits
        .as_ref()
        .filter(|credits| credits.known)
        .map(|credits| credits.summary.clone());
    if let Some(spend) = value("Local spend (this machine)") {
        return (Some(spend.to_owned()), plan);
    }
    // A period with no dollar figure (unpriced model, no recording) is left out
    // rather than shown as "n/a".
    let period = |key: &str| {
        let text = value(key)?;
        let amount = if text.starts_with("No recorded usage") {
            "$0.00".to_owned()
        } else {
            dollars(text)?
        };
        Some(format!("{} {amount}", key.to_lowercase()))
    };
    let spend: Vec<String> = [period("Today"), period("Lifetime")]
        .into_iter()
        .flatten()
        .collect();
    let spend = (!spend.is_empty()).then(|| format!("{} (est.)", spend.join(" · ")));
    let usage = [balance, spend].into_iter().flatten().collect::<Vec<_>>();
    ((!usage.is_empty()).then(|| usage.join(" · ")), plan)
}

pub(super) fn build_rows(
    providers: &[LoginProvider],
    statuses: Option<&ConnectionStatuses>,
    loading: bool,
    testing: &dyn Fn(&str) -> bool,
    data: &AccountsData,
) -> Vec<AccountRow> {
    let mut rows = Vec::new();
    for provider in providers {
        let mut status = status_for(statuses, provider.id, loading);
        let account = data
            .accounts
            .as_ref()
            .and_then(|accounts| accounts.iter().find(|a| a.id == provider.id));
        // Auth status saw a credential the health report did not check.
        if status == ConnectionStatus::NotConnected
            && account.is_some_and(|a| a.status == "available")
        {
            status = ConnectionStatus::Unverified;
        }
        // A fresh, successful quota fetch is an authenticated provider call,
        // so a logged-in subscription with live usage is not "untested".
        if status == ConnectionStatus::Unverified && account.is_some_and(proven_by_usage) {
            status = ConnectionStatus::Connected;
        }
        // A rejected credential is a failure we already know without spending
        // a model request, so the row says "Needs attention" instead of
        // "Untested" and carries the provider's reason.
        if status == ConnectionStatus::Unverified && account.is_some_and(rejected_by_usage) {
            status = ConnectionStatus::Failed;
        }
        if status != ConnectionStatus::NotConnected && testing(provider.id) {
            status = ConnectionStatus::Testing;
        }
        let logins: Vec<&OAuthLogin> = if MULTI_ACCOUNT.contains(&provider.id) {
            data.logins
                .iter()
                .filter(|login| login.provider == provider.id)
                .collect()
        } else {
            Vec::new()
        };
        let row = |label: Option<&str>, email: Option<String>, active: bool, first: bool| {
            let report = account.and_then(|a| report_for(a, label, logins.len()));
            let limits = match (report, account) {
                (Some(report), _) => report.limits.clone(),
                (None, Some(account)) if label.is_none() => account.limits.clone(),
                _ => Vec::new(),
            };
            let (usage, plan) = report.map(usage_summary).unwrap_or_default();
            let error = report.and_then(|report| report.usage_error());
            // A confirmed zero is useful information. Only missing provider
            // data is unknown, not an empty reset bank.
            let banked = report.and_then(|r| r.banked_reset.clone());
            AccountRow {
                key: account_key(provider.id, label),
                provider: *provider,
                label: label.map(str::to_owned),
                email,
                name: report
                    .and_then(|report| report.identity.as_ref())
                    .map(|identity| identity.display_name.clone()),
                active,
                status,
                limits,
                usage,
                plan,
                banked,
                configured: account.is_some_and(|a| a.status != "not_configured"),
                error,
                first_of_provider: first,
            }
        };
        if logins.is_empty() {
            rows.push(row(None, None, false, true));
        } else {
            for (index, login) in logins.iter().enumerate() {
                rows.push(row(
                    Some(&login.label),
                    login.email.as_deref().map(mask_email),
                    login.active && logins.len() > 1,
                    index == 0,
                ));
            }
        }
    }
    rows
}

/// The account's active usage report came back from the provider just now
/// (not a cached copy, no fetch error) with real quota numbers.
fn proven_by_usage(account: &Account) -> bool {
    let Some(report) = account.active_report() else {
        return false;
    };
    let cached = report
        .extra_info
        .iter()
        .any(|(key, value)| key == "Last updated" && value.contains("cached"));
    !cached
        && report.usage_error().is_none()
        && report
            .limits
            .iter()
            .any(|limit| limit.usage_percent.is_finite())
}

/// The provider answered a fresh usage probe by rejecting the credential. That
/// is proof the key does not work, without spending a model request.
fn rejected_by_usage(account: &Account) -> bool {
    let Some(report) = account.active_report() else {
        return false;
    };
    let cached = report
        .extra_info
        .iter()
        .any(|(key, value)| key == "Last updated" && value.contains("cached"));
    !cached && report.auth_failure()
}

/// Providers offered only once configured. The Jcode subscription is sold
/// separately, so an unconfigured entry is noise in the Connect list.
const HIDDEN_UNTIL_CONFIGURED: [&str; 1] = ["jcode"];

/// Rows the composer's provider picker lists: connected credentials only,
/// narrowed by the search box. Shared by the picker and its tests so "which
/// providers can I switch to" has one answer.
pub(super) fn picker_rows(rows: Vec<AccountRow>, query: &str) -> Vec<AccountRow> {
    let query = query.trim().to_lowercase();
    rows.into_iter()
        .filter(AccountRow::connected)
        .filter(|row| {
            format!(
                "{} {} {} {} {}",
                row.provider.display_name,
                row.provider.id,
                row.provider.detail,
                jcode_desktop_accounts_ui::catalog::method_label(row.provider.method),
                row.label.as_deref().unwrap_or(""),
            )
            .to_lowercase()
            .contains(&query)
        })
        .collect()
}

pub(super) fn group_rows(rows: Vec<AccountRow>, pool: &AccountPool) -> Groups {
    let (connected, available): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(AccountRow::connected);
    let available: Vec<_> = available
        .into_iter()
        .filter(|row| !HIDDEN_UNTIL_CONFIGURED.contains(&row.provider.id))
        .collect();
    let keys: Vec<(String, bool)> = connected
        .iter()
        .map(|row| (row.key.clone(), row.default_pooled()))
        .collect();
    let (member_keys, manual_keys) = pool.partition(&keys);
    let take = |keys: &[&str]| -> Vec<AccountRow> {
        keys.iter()
            .filter_map(|key| connected.iter().find(|row| row.key == *key).cloned())
            .collect()
    };
    let mut addable = Vec::new();
    for row in &connected {
        if MULTI_ACCOUNT.contains(&row.provider.id)
            && !addable
                .iter()
                .any(|p: &LoginProvider| p.id == row.provider.id)
        {
            addable.push(row.provider);
        }
    }
    Groups {
        pooled: take(&member_keys),
        manual: take(&manual_keys),
        available,
        addable,
    }
}

fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return email.to_owned();
    };
    let mut chars = local.chars();
    match (chars.next(), chars.last()) {
        (Some(first), Some(last)) => format!("{first}***{last}@{domain}"),
        (Some(first), None) => format!("{first}***@{domain}"),
        _ => email.to_owned(),
    }
}

/// Offline data for previews, screenshots and tests. Never touches credentials.
pub(super) fn offline_data() -> AccountsData {
    let limit = |name: &str, used: f32, reset: &str| UsageLimit {
        name: name.into(),
        usage_percent: used,
        reset_in: Some(reset.into()),
    };
    let banked = |provider, label: &str, count, limit_reached, next: Option<&str>| {
        // "soon" means about 3h from now, so screenshots read realistically.
        let next = next.map(|next| match next {
            "soon" => {
                let at =
                    std::time::SystemTime::now() + std::time::Duration::from_secs(3 * 3600 + 120);
                httpdate_rfc3339(at)
            }
            other => other.to_owned(),
        });
        Some(BankedReset {
            provider,
            account_label: Some(label.into()),
            available_count: count,
            usable_count: None,
            expires_at: None,
            cached_source: None,
            session_reset_available: None,
            limit_reached,
            next_available_at: next,
        })
    };
    let report = |label: &str, limits: Vec<UsageLimit>, today: &str| {
        UsageReport { identity: None, credits: None,
        provider_name: format!("OpenAI (ChatGPT) ({label})"),
        account_label: Some(label.into()),
        limits,
        banked_reset: match label {
            "openai-otter" => banked(ResetProvider::OpenAi, label, 2, true, None),
            _ => banked(ResetProvider::OpenAi, label, 1, false, None),
        },
        extra_info: vec![
            ("Plan".into(), "plus".into()),
            ("Today".into(), format!("{today} API-equivalent estimate, not a bill")),
            ("Lifetime".into(), "2400000 input / 160000 output tokens, $84.1000 API-equivalent estimate, not a bill".into()),
        ],
    }
    };
    let account = |id: &str, name: &str, reports: Vec<UsageReport>| Account {
        id: id.into(),
        display_name: name.into(),
        status: "available".into(),
        auth_kind: "OAuth".into(),
        method: "Offline fixture".into(),
        limits: reports.iter().flat_map(|r| r.limits.clone()).collect(),
        usage_reports: reports,
    };
    let login = |provider, label: &str, email: &str, active| OAuthLogin {
        provider,
        label: label.into(),
        email: Some(email.into()),
        active,
    };
    AccountsData {
        accounts: Some(vec![
            account(
                "openai",
                "OpenAI",
                vec![
                    report(
                        "openai-otter",
                        vec![
                            limit("5-hour window", 92., "41m"),
                            limit("7-day window", 66., "3d 20h"),
                        ],
                        "120000 input tokens, $4.2000",
                    ),
                    report(
                        "openai-fox",
                        vec![
                            limit("5-hour window", 8., "4h 2m"),
                            limit("7-day window", 21., "5d"),
                        ],
                        "No recorded usage",
                    ),
                ],
            ),
            account(
                "claude",
                "Anthropic/Claude",
                vec![UsageReport {
                    identity: None,
                    credits: None,
                    provider_name: "Anthropic (Claude) (claude-otter)".into(),
                    account_label: None,
                    limits: vec![
                        limit("5-hour window", 35., "2h"),
                        limit("7-day window", 48., "4d"),
                    ],
                    banked_reset: banked(
                        ResetProvider::Claude,
                        "claude-otter",
                        0,
                        false,
                        Some("soon"),
                    ),
                    extra_info: vec![("Plan".into(), "max".into())],
                }],
            ),
            Account {
                id: "opencode-go".into(),
                display_name: "OpenCode Go".into(),
                status: "available".into(),
                auth_kind: "API key".into(),
                method: "API key (`OPENCODE_GO_API_KEY`)".into(),
                limits: Vec::new(),
                usage_reports: vec![UsageReport {
                    identity: None,
                    credits: None,
                    provider_name: "OpenCode Go (API key)".into(),
                    account_label: None,
                    limits: vec![
                        limit("Rolling window", 38., "2h 56m"),
                        limit("Weekly window", 15., "15h 48m"),
                        limit("Monthly window", 7., "29d 21h"),
                    ],
                    banked_reset: None,
                    extra_info: vec![("Key status".into(), "valid".into())],
                }],
            },
            Account {
                id: "openai-api".into(),
                display_name: "OpenAI API".into(),
                status: "available".into(),
                auth_kind: "API key".into(),
                method: "Offline fixture".into(),
                limits: Vec::new(),
                usage_reports: vec![UsageReport {
                    identity: None,
                    credits: None,
                    provider_name: "OpenAI API key".into(),
                    account_label: None,
                    limits: Vec::new(),
                    banked_reset: None,
                    extra_info: vec![(
                        "Local spend (this machine)".into(),
                        "$3.10 today · $41.20 this month".into(),
                    )],
                }],
            },
        ]),
        logins: vec![
            login("openai", "openai-otter", "jeremy@personal.dev", true),
            login("openai", "openai-fox", "jeremy@work.dev", false),
            login("claude", "claude-otter", "jeremy@personal.dev", true),
        ],
        // Fixture labels can also exist in the real credential store. Pin their
        // priorities so previews and tests never depend on local plan metadata.
        pool: AccountPool {
            priorities: [
                "openai:openai-otter",
                "openai:openai-fox",
                "claude:claude-otter",
                "openai-api",
            ]
            .into_iter()
            .map(|key| (key.to_owned(), 1))
            .collect(),
            ..AccountPool::default()
        },
        default_route: None,
        default_key: None,
    }
}

/// UTC RFC3339 for a wall-clock time (offline fixtures only).
fn httpdate_rfc3339(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// What the reset chip says: how many saved resets can be used right now.
pub(super) fn reset_pill_label(reset: &BankedReset) -> String {
    format!("Use saved reset ({})", reset.usable())
}

/// What clicking a row in the composer's provider picker would do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickerChoice {
    /// This session already runs on the provider.
    Current,
    /// Switches the session to the provider.
    Switch,
    /// The row cannot take the session, with the reason shown on it.
    Unavailable(&'static str),
}

/// The picker's decision for one row: active wins, then a credential that is
/// known not to work, then whether the session catalog has a route at all.
/// Pure so the A -> B -> A switch cycle is testable without a window.
pub(super) fn picker_choice_for(
    session_provider: Option<&str>,
    row_provider: &str,
    status: ConnectionStatus,
    has_route: bool,
) -> PickerChoice {
    if session_provider == Some(row_provider) {
        return PickerChoice::Current;
    }
    match status {
        ConnectionStatus::NotConnected => PickerChoice::Unavailable("Not connected"),
        ConnectionStatus::Failed => PickerChoice::Unavailable("Key rejected"),
        _ if !has_route => PickerChoice::Unavailable("No model for this session"),
        _ => PickerChoice::Switch,
    }
}

/// Login provider id for the session's serving route.
pub(super) fn session_provider_id(provider: Option<&str>, auth: Option<&str>) -> Option<String> {
    let provider = provider?.trim().to_ascii_lowercase();
    let api_key = auth.is_some_and(|auth| {
        let auth = auth.to_ascii_lowercase();
        auth.contains("api") || auth.contains("key")
    });
    Some(match (provider.as_str(), api_key) {
        ("claude" | "anthropic", false) => "claude".into(),
        ("claude" | "anthropic", true) => "anthropic-api".into(),
        ("openai" | "chatgpt", false) => "openai".into(),
        ("openai" | "chatgpt", true) => "openai-api".into(),
        (other, _) => other.replace(' ', "-"),
    })
}

impl super::LoginState {
    /// Rows for `providers`, with any in-flight live test shown as Testing.
    pub(super) fn account_rows(&self, providers: &[LoginProvider]) -> Vec<AccountRow> {
        let testing = |id: &str| self.live_test_all || self.live_testing.contains(id);
        build_rows(
            providers,
            self.statuses.as_ref(),
            self.status_loading,
            &testing,
            &self.accounts,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::account_rows::short_limit_name;
    use super::*;
    use crate::accounts::Credits;

    fn provider(id: &'static str, method: LoginMethod) -> LoginProvider {
        LoginProvider {
            id,
            display_name: id,
            detail: "",
            method,
        }
    }

    #[test]
    fn live_usage_probes_resolve_untested_keys_without_spending_a_request() {
        let providers = [
            provider("opencode-go", LoginMethod::ApiKey),
            provider("openrouter", LoginMethod::ApiKey),
        ];
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[
                {"id":"opencode-go","status":"available","auth_kind":"API key"},
                {"id":"openrouter","status":"available","auth_kind":"API key"}
            ]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[
                {"provider_name":"OpenCode Go (API key)","limits":[
                    {"name":"Rolling window","usage_percent":38.0,"reset_in":"2h 56m"}
                ],"extra_info":[],"error":null},
                {"provider_name":"OpenRouter","limits":[],
                 "extra_info":[],"error":"OpenRouter rejected the API key (401 Unauthorized)"}
            ]}"#,
        );
        let data = AccountsData {
            accounts: Some(accounts),
            ..Default::default()
        };
        // Both are saved credentials with no recent live test.
        let statuses = ConnectionStatuses::from([
            ("opencode-go".into(), ConnectionStatus::Unverified),
            ("openrouter".into(), ConnectionStatus::Unverified),
        ]);
        let rows = build_rows(&providers, Some(&statuses), false, &|_| false, &data);
        let go = rows
            .iter()
            .find(|row| row.provider.id == "opencode-go")
            .unwrap();
        assert_eq!(
            go.status,
            ConnectionStatus::Connected,
            "a live quota answer proves the key"
        );
        assert_eq!(go.limits.len(), 1);
        let router = rows
            .iter()
            .find(|row| row.provider.id == "openrouter")
            .unwrap();
        assert_eq!(
            router.status,
            ConnectionStatus::Failed,
            "a rejected key is not merely untested"
        );
        let reason = router.error.as_deref().unwrap_or_default();
        assert!(reason.contains("Sign in again"), "{reason}");
    }

    #[test]
    fn rate_limits_do_not_masquerade_as_a_rejected_key() {
        let providers = [provider("openrouter", LoginMethod::ApiKey)];
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[{"id":"openrouter","status":"available","auth_kind":"API key"}]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[{"provider_name":"OpenRouter","limits":[],
                "extra_info":[],"error":"429 Too Many Requests"}]}"#,
        );
        let data = AccountsData {
            accounts: Some(accounts),
            ..Default::default()
        };
        let statuses =
            ConnectionStatuses::from([("openrouter".into(), ConnectionStatus::Unverified)]);
        let rows = build_rows(&providers, Some(&statuses), false, &|_| false, &data);
        assert_eq!(rows[0].status, ConnectionStatus::Unverified);
    }

    #[test]
    fn provider_picker_lists_configured_opencode_go_with_its_quota() {
        let providers = [LoginProvider {
            id: "opencode-go",
            display_name: "OpenCode Go",
            detail: "API key",
            method: LoginMethod::ApiKey,
        }];
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[{"id":"opencode-go","display_name":"OpenCode Go","status":"available",
                "method":"API key (`OPENCODE_GO_API_KEY`)","auth_kind":"API key"}]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[{"provider_name":"OpenCode Go (API key)","limits":[
                {"name":"Rolling window","usage_percent":23.0,"reset_in":"4h"},
                {"name":"Weekly window","usage_percent":9.0,"reset_in":"1d"},
                {"name":"Monthly window","usage_percent":4.0,"reset_in":"30d"}
            ],"extra_info":[],"error":null}]}"#,
        );
        let data = AccountsData {
            accounts: Some(accounts),
            ..Default::default()
        };
        let rows = build_rows(&providers, None, false, &|_| false, &data);
        let listed = picker_rows(rows, "");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].connected());
        assert_eq!(listed[0].provider.id, "opencode-go");
        assert_eq!(listed[0].limits.len(), 3);
        assert!(
            crate::accounts::logo(listed[0].provider.id).is_none(),
            "OpenCode takes the lettermark, not a bare frame"
        );
        // The search field matches the provider by name, id and method.
        let providers = [providers[0]];
        for query in ["opencode", "OPENCODE-GO", "api key"] {
            let rows = build_rows(&providers, None, false, &|_| false, &data);
            assert_eq!(picker_rows(rows, query).len(), 1, "query {query}");
        }
    }

    #[test]
    fn live_quota_proves_auth_and_unconfigured_jcode_is_hidden() {
        let providers = [
            provider("claude", LoginMethod::OAuth),
            provider("openai", LoginMethod::OAuth),
            provider("jcode", LoginMethod::ApiKey),
        ];
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[{"id":"claude","status":"available"},{"id":"openai","status":"available"},{"id":"jcode","status":"not_configured"}]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[
                {"provider_name":"Anthropic (Claude) (x***g@example.com)","limits":[{"name":"7-day window","usage_percent":28.0,"reset_in":"1d"}],"extra_info":[],"error":null},
                {"provider_name":"OpenAI (ChatGPT) (m*@example.com)","limits":[],"extra_info":[],"error":"401 Unauthorized"}
            ]}"#,
        );
        let data = AccountsData {
            accounts: Some(accounts),
            ..Default::default()
        };
        // Doctor saw saved credentials but no recent probe for either login.
        let statuses: ConnectionStatuses = [
            ("claude".to_string(), ConnectionStatus::Unverified),
            ("openai".to_string(), ConnectionStatus::Unverified),
        ]
        .into();
        let rows = build_rows(&providers, Some(&statuses), false, &|_| false, &data);
        let status = |id: &str| rows.iter().find(|r| r.provider.id == id).unwrap().status;
        assert_eq!(status("claude"), ConnectionStatus::Connected);
        // A rejected credential is not merely untested: the provider answered
        // the probe by refusing it, so the row asks for attention.
        assert_eq!(status("openai"), ConnectionStatus::Failed);
        let groups = group_rows(rows, &AccountPool::default());
        assert!(
            groups
                .available
                .iter()
                .all(|row| row.provider.id != "jcode"),
            "unconfigured Jcode subscription is not offered"
        );
    }

    #[test]
    fn session_provider_ids_match_login_rows() {
        assert_eq!(
            session_provider_id(Some("claude"), Some("oauth")).as_deref(),
            Some("claude")
        );
        assert_eq!(
            session_provider_id(Some("anthropic"), Some("api key")).as_deref(),
            Some("anthropic-api")
        );
        assert_eq!(
            session_provider_id(Some("openai"), Some("oauth")).as_deref(),
            Some("openai")
        );
        assert_eq!(
            session_provider_id(Some("openai"), Some("api_key")).as_deref(),
            Some("openai-api")
        );
        assert_eq!(session_provider_id(None, Some("oauth")), None);
    }

    #[test]
    fn logins_become_rows_and_defaults_split_subscriptions_from_keys() {
        let providers = [
            provider("openai", LoginMethod::OAuth),
            provider("openai-api", LoginMethod::ApiKey),
            provider("claude", LoginMethod::OAuth),
            provider("cursor", LoginMethod::ApiKey),
        ];
        let statuses = ConnectionStatuses::from([
            ("openai".into(), ConnectionStatus::Connected),
            ("openai-api".into(), ConnectionStatus::Unverified),
            ("claude".into(), ConnectionStatus::Expired),
        ]);
        let data = offline_data();
        let rows = build_rows(&providers, Some(&statuses), false, &|_| false, &data);
        let keys: Vec<_> = rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "openai:openai-otter",
                "openai:openai-fox",
                "openai-api",
                "claude:claude-otter",
                "cursor"
            ]
        );
        let otter = &rows[0];
        assert!(otter.active);
        assert_eq!(otter.email.as_deref(), Some("j***y@personal.dev"));
        assert_eq!(otter.limits.len(), 2);
        assert_eq!(otter.limits[0].usage_percent, 92.);
        assert_eq!(
            otter.usage.as_deref(),
            Some("today $4.20 · lifetime $84.10 (est.)")
        );
        assert_eq!(otter.plan.as_deref(), Some("plus"));
        assert_eq!(
            rows[1].usage.as_deref(),
            Some("today $0.00 · lifetime $84.10 (est.)")
        );
        assert_eq!(
            rows[2].usage.as_deref(),
            Some("$3.10 today · $41.20 this month")
        );
        // One Claude login: its single report is attributed to it.
        assert_eq!(rows[3].limits.len(), 2);

        let groups = group_rows(rows, &data.pool);
        let pooled: Vec<_> = groups.pooled.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            pooled,
            [
                "openai:openai-otter",
                "openai:openai-fox",
                "claude:claude-otter"
            ]
        );
        assert_eq!(groups.manual[0].key, "openai-api");
        assert_eq!(groups.available[0].key, "cursor");
        let addable: Vec<_> = groups.addable.iter().map(|p| p.id).collect();
        assert_eq!(addable, ["openai", "claude"]);
    }

    #[gpui::test]
    fn reorder_controls_and_dragging_move_accounts_without_opening_sign_in(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("pool-test", None, crate::harness::spawn_inert(), cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-group-auto").is_some());
        assert!(vcx.debug_bounds("login-group-manual").is_some());
        assert!(vcx.debug_bounds("login-add-openai").is_some());
        let pooled = |vcx: &mut gpui::VisualTestContext| {
            panel.read_with(vcx, |panel, _| {
                let state = panel.login.as_ref().unwrap();
                let providers = super::super::catalog::filtered_providers(
                    &state.providers,
                    "",
                    state.statuses.as_ref(),
                    state.status_loading,
                    state.usage.as_ref(),
                );
                let rows = state.account_rows(&providers);
                group_rows(rows, &state.accounts.pool)
                    .pooled
                    .into_iter()
                    .map(|row| row.key)
                    .collect::<Vec<_>>()
            })
        };
        let before = pooled(vcx);
        assert!(before.contains(&"openai:openai-otter".to_string()));
        assert!(!before.contains(&"openai-api".to_string()));

        let handle = vcx
            .debug_bounds("login-drag-handle-openai:openai-otter")
            .expect("connected accounts expose a visible grip");
        vcx.simulate_click(handle.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(pooled(vcx), before);
        assert!(panel.read_with(vcx, |panel, _| {
            panel.login.as_ref().unwrap().provider.is_none()
        }));

        let click_order = |vcx: &mut gpui::VisualTestContext, direction: &str, key: &str| {
            let selector = Box::leak(format!("login-order-{direction}-{key}").into_boxed_str());
            let bounds = vcx.debug_bounds(selector).unwrap();
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        // Boundary controls do nothing, and clicking never opens sign-in.
        click_order(vcx, "up", &before[0]);
        click_order(vcx, "down", before.last().unwrap());
        assert_eq!(pooled(vcx), before);
        // Move down through the middle to the end, then back to the top.
        for index in 1..before.len() {
            click_order(vcx, "down", &before[0]);
            assert_eq!(pooled(vcx)[index], before[0]);
        }
        for index in (0..before.len() - 1).rev() {
            click_order(vcx, "up", &before[0]);
            assert_eq!(pooled(vcx)[index], before[0]);
        }
        assert_eq!(pooled(vcx), before);
        assert!(panel.read_with(vcx, |panel, _| {
            let state = panel.login.as_ref().unwrap();
            state.provider.is_none() && state.accounts.default_key.as_ref() == Some(&before[0])
        }));
        assert!(vcx.debug_bounds("login-order-up-openai-api").is_none());

        let drag = |vcx: &mut gpui::VisualTestContext, from: &'static str, to: &'static str| {
            let from = vcx.debug_bounds(from).unwrap().center();
            let to = vcx.debug_bounds(to).unwrap().center();
            let left = gpui::MouseButton::Left;
            vcx.simulate_mouse_down(from, left, gpui::Modifiers::default());
            vcx.simulate_mouse_move(
                from + point(px(0.), px(12.)),
                left,
                gpui::Modifiers::default(),
            );
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("login-account-drag-preview").is_some());
            let midway = from + (to - from) * 0.5;
            vcx.simulate_mouse_move(midway, left, gpui::Modifiers::default());
            vcx.run_until_parked();
            let midway_preview = vcx.debug_bounds("login-account-drag-preview").unwrap();
            vcx.simulate_mouse_move(to, left, gpui::Modifiers::default());
            vcx.run_until_parked();
            let final_preview = vcx.debug_bounds("login-account-drag-preview").unwrap();
            let actual = final_preview.origin - midway_preview.origin;
            let expected = to - midway;
            // Layout snaps painted bounds to device pixels, while pointer
            // coordinates can land between pixels on fractional row centers.
            assert!(f32::from(actual.x - expected.x).abs() <= 1.0);
            assert!(f32::from(actual.y - expected.y).abs() <= 1.0);
            vcx.simulate_mouse_up(to, left, gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        drag(
            vcx,
            "login-drag-handle-openai:openai-otter",
            "login-provider-openai",
        );
        assert_eq!(pooled(vcx), before);
        // API key (manual) dropped onto the first auto-switch row goes first.
        drag(vcx, "login-drag-handle-openai-api", "login-provider-openai");
        let after = pooled(vcx);
        assert_eq!(after[0], "openai-api");
        assert_eq!(after[1..], before[..]);
        // Reorder: the second OpenAI login moves ahead of the first.
        drag(
            vcx,
            "login-drag-handle-openai:openai-fox",
            "login-provider-openai-api",
        );
        assert_eq!(pooled(vcx)[0], "openai:openai-fox");
        // Out of rotation: drop on the Manual group.
        drag(
            vcx,
            "login-provider-openai-openai-fox",
            "login-group-manual",
        );
        assert!(!pooled(vcx).contains(&"openai:openai-fox".to_string()));
        // Move back by dropping onto the Auto-switch section, without a toggle pill.
        assert!(vcx.debug_bounds("login-move-openai:openai-fox").is_none());
        drag(
            vcx,
            "login-drag-handle-openai:openai-fox",
            "login-group-auto",
        );
        assert!(pooled(vcx).contains(&"openai:openai-fox".to_string()));
        assert!(panel.read_with(vcx, |panel, _| {
            panel.login.as_ref().unwrap().provider.is_none()
        }));
        drag(
            vcx,
            "login-drag-handle-openai:openai-fox",
            "login-group-manual",
        );
        assert!(!pooled(vcx).contains(&"openai:openai-fox".to_string()));
    }

    #[gpui::test]
    fn live_test_controls_mark_rows_testing_without_opening_sign_in(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("live-test", None, crate::harness::spawn_inert(), cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-test-all").is_some());
        assert!(vcx.debug_bounds("login-default-account").is_some());
        let test = vcx
            .debug_bounds("login-test-openai:openai-otter")
            .expect("connected rows offer a Test pill")
            .center();
        // Clicking Test marks the provider testing and never opens sign-in.
        panel.update(vcx, |panel, cx| {
            panel.live_test_providers(Some("openai".into()), cx);
            let state = panel.login.as_ref().unwrap();
            let rows = state.account_rows(&state.providers);
            assert!(
                rows.iter()
                    .filter(|row| row.provider.id == "openai")
                    .all(|row| row.status == ConnectionStatus::Testing)
            );
        });
        vcx.run_until_parked();
        vcx.simulate_click(test, gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let state = panel.login.as_ref().unwrap();
            assert!(state.provider.is_none());
            assert!(state.live_testing.is_empty());
        });
    }

    #[gpui::test]
    fn banked_resets_show_per_login_and_click_only_requests_review(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("reset-test", None, crate::harness::spawn_inert(), cx)
        });
        vcx.run_until_parked();
        let requested = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        vcx.update(|_, cx| {
            let requested = requested.clone();
            cx.subscribe(&panel, move |_, event: &AccountsPanelRedeemReset, _| {
                requested.borrow_mut().push(event.0.clone());
            })
            .detach();
        });
        // Both OpenAI logins hold a saved reset the provider reports usable.
        assert!(vcx.debug_bounds("login-reset-openai:openai-fox").is_some());
        // The Claude login holds none: no chip at all, not an "unavailable" one.
        assert!(
            vcx.debug_bounds("login-reset-claude:claude-otter")
                .is_none()
        );
        // Clicking only requests the review, never spends: that stays with the
        // confirmed dialog. The limit not binding does not make the chip dead.
        let fox = vcx
            .debug_bounds("login-reset-openai:openai-fox")
            .unwrap()
            .center();
        vcx.simulate_click(fox, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(requested.borrow().len(), 1);
        assert_eq!(
            requested.borrow()[0].account_label.as_deref(),
            Some("openai-fox")
        );
        requested.borrow_mut().clear();
        let otter = vcx
            .debug_bounds("login-reset-openai:openai-otter")
            .unwrap()
            .center();
        vcx.simulate_click(otter, gpui::Modifiers::default());
        vcx.run_until_parked();
        let requested = requested.borrow();
        assert_eq!(requested.len(), 1);
        assert_eq!(requested[0].account_label.as_deref(), Some("openai-otter"));
        // The pill does not also open the provider's sign-in flow.
        assert!(panel.read_with(vcx, |panel, _| {
            panel.login.as_ref().unwrap().provider.is_none()
        }));
    }

    #[gpui::test]
    fn a_saved_claude_reset_below_the_wall_is_a_working_button(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("saved-reset-test", None, crate::harness::spawn_inert(), cx)
        });
        vcx.run_until_parked();
        let requested = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        vcx.update(|_, cx| {
            let requested = requested.clone();
            cx.subscribe(&panel, move |_, event: &AccountsPanelRedeemReset, _| {
                requested.borrow_mut().push(event.0.clone());
            })
            .detach();
        });
        let set_reset = |vcx: &mut gpui::VisualTestContext, available, usable| {
            panel.update(vcx, |panel, cx| {
                let account = panel
                    .login
                    .as_mut()
                    .unwrap()
                    .accounts
                    .accounts
                    .as_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|account| account.id == "claude")
                    .unwrap();
                // As `jcode usage --json` reports it: no session reset on offer,
                // the limit not reached, saved grants as counted.
                account.usage_reports[0].banked_reset = Some(BankedReset {
                    provider: ResetProvider::Claude,
                    account_label: Some("claude-otter".into()),
                    available_count: available,
                    usable_count: Some(usable),
                    expires_at: Some("2099-03-09T07:59:00Z".into()),
                    cached_source: None,
                    session_reset_available: Some(false),
                    limit_reached: false,
                    next_available_at: None,
                });
                cx.notify();
            });
            vcx.run_until_parked();
        };
        // Saved but none usable: nothing to show or click.
        set_reset(vcx, 3, 0);
        assert!(
            vcx.debug_bounds("login-reset-claude:claude-otter")
                .is_none()
        );
        // One reported usable: the chip appears and asks for the review.
        set_reset(vcx, 1, 1);
        let chip = vcx
            .debug_bounds("login-reset-claude:claude-otter")
            .expect("a usable saved reset shows its chip");
        vcx.simulate_click(chip.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let requested = requested.borrow();
        assert_eq!(requested.len(), 1);
        assert_eq!(requested[0].account_label.as_deref(), Some("claude-otter"));
        assert_eq!(requested[0].usable_count, Some(1));
        assert_eq!(reset_pill_label(&requested[0]), "Use saved reset (1)");
    }

    #[test]
    fn the_usage_line_leads_with_a_stated_balance_and_never_says_n_a() {
        let report = |credits: Option<Credits>, extra: &[(&str, &str)]| UsageReport {
            identity: None,
            credits,
            provider_name: "x".into(),
            account_label: None,
            limits: Vec::new(),
            banked_reset: None,
            extra_info: extra
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        };
        let balance = Credits {
            known: true,
            summary: "45001.54 credits left".into(),
            used_fraction: None,
        };
        // ChatGPT: a balance, today unpriced, lifetime partly priced.
        let chatgpt = report(
            Some(balance.clone()),
            &[
                ("Today", "109,441 in · 729 out · cost unknown"),
                (
                    "Lifetime",
                    "11.498 Bil in · $259.64 est. + unknown cost (78,122 unpriced)",
                ),
            ],
        );
        assert_eq!(
            usage_summary(&chatgpt).0.as_deref(),
            Some("45001.54 credits left · lifetime $259.64 (est.)")
        );
        // Nothing priced at all: the balance alone, not "today n/a · lifetime n/a".
        let unpriced = report(
            Some(balance),
            &[
                ("Today", "1 in · cost unknown"),
                ("Lifetime", "2 in · cost unknown"),
            ],
        );
        assert_eq!(
            usage_summary(&unpriced).0.as_deref(),
            Some("45001.54 credits left")
        );
        // An unknown balance is not a figure, and no data is no line.
        let unknown = Credits {
            known: false,
            summary: "unknown".into(),
            used_fraction: None,
        };
        assert_eq!(usage_summary(&report(Some(unknown), &[])).0, None);
        assert_eq!(
            usage_summary(&report(None, &[("Today", "No recorded usage")]))
                .0
                .as_deref(),
            Some("today $0.00 (est.)")
        );
    }

    #[test]
    fn picker_switches_a_to_b_to_a_and_blocks_providers_without_a_working_key() {
        use ConnectionStatus::*;
        // The session's provider id follows each completed switch, as the
        // daemon's ModelInfo does. Every other connected row stays clickable.
        let mut session = "claude";
        for target in ["opencode-go", "openai", "claude", "opencode-go", "claude"] {
            assert_eq!(
                picker_choice_for(Some(session), target, Connected, true),
                PickerChoice::Switch,
                "{session} -> {target}"
            );
            assert_eq!(
                picker_choice_for(Some(session), session, Connected, true),
                PickerChoice::Current
            );
            session = target;
        }
        // A credential that is known not to work cannot take the session.
        for (status, reason) in [(NotConnected, "Not connected"), (Failed, "Key rejected")] {
            assert_eq!(
                picker_choice_for(Some("claude"), "openrouter", status, true),
                PickerChoice::Unavailable(reason)
            );
        }
        // Untested keys and expired OAuth (refreshed on use) stay selectable.
        for status in [Unverified, Expired] {
            assert_eq!(
                picker_choice_for(Some("claude"), "opencode-go", status, true),
                PickerChoice::Switch
            );
        }
        // Session provider labels map onto login row ids.
        assert_eq!(
            session_provider_id(Some("OpenCode Go"), Some("API key")).as_deref(),
            Some("opencode-go")
        );
    }

    #[test]
    fn confirmed_zero_resets_are_retained_but_missing_counts_are_not_invented() {
        let providers = [provider("claude", LoginMethod::OAuth)];
        let mut data = offline_data();
        let report = data
            .accounts
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|account| account.id == "claude")
            .unwrap()
            .usage_reports
            .first_mut()
            .unwrap();
        report.banked_reset = Some(BankedReset {
            provider: ResetProvider::Claude,
            account_label: Some("claude-otter".into()),
            available_count: 0,
            usable_count: None,
            expires_at: None,
            cached_source: None,
            session_reset_available: None,
            limit_reached: false,
            next_available_at: None,
        });
        let rows = build_rows(&providers, None, false, &|_| false, &data);
        assert_eq!(rows[0].banked.as_ref().unwrap().available_count, 0);
        data.accounts
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|account| account.id == "claude")
            .unwrap()
            .usage_reports
            .first_mut()
            .unwrap()
            .banked_reset = None;
        let rows = build_rows(&providers, None, false, &|_| false, &data);
        assert!(rows[0].banked.is_none());
    }

    #[test]
    fn the_reset_chip_counts_what_can_be_used_right_now() {
        let reset = |provider, count, usable, session| BankedReset {
            provider,
            account_label: None,
            available_count: count,
            usable_count: usable,
            expires_at: None,
            cached_source: None,
            session_reset_available: session,
            limit_reached: false,
            next_available_at: None,
        };
        // Real shape: one of two grants spent, the other reported usable, and
        // no session reset on offer. The count is what the click will spend.
        let saved = reset(ResetProvider::Claude, 1, Some(1), Some(false));
        assert_eq!(reset_pill_label(&saved), "Use saved reset (1)");
        // Saved but none usable now (paused, cooling down): nothing to click.
        assert_eq!(
            reset(ResetProvider::Claude, 3, Some(0), Some(false)).usable(),
            0
        );
        assert_eq!(
            reset(ResetProvider::Claude, 0, Some(0), Some(false)).usable(),
            0
        );
        // The weekly session reset counts as one when it is on offer.
        assert_eq!(
            reset(ResetProvider::Claude, 1, Some(0), Some(true)).usable(),
            1
        );
        assert_eq!(
            reset(ResetProvider::Claude, 1, None, Some(true)).usable(),
            1
        );
        assert_eq!(reset(ResetProvider::Claude, 2, None, None).usable(), 0);
        assert_eq!(reset(ResetProvider::OpenAi, 2, None, None).usable(), 2);
        assert_eq!(reset(ResetProvider::OpenAi, 0, None, None).usable(), 0);
    }

    #[test]
    fn usage_figures_are_trimmed_to_cents() {
        assert_eq!(dollars("x, $2693.5357 known"), Some("$2693.54".into()));
        assert_eq!(dollars("no amount"), None);
        assert_eq!(short_limit_name("5-hour window"), "5h");
        assert_eq!(short_limit_name("7-day window"), "7d");
        assert_eq!(mask_email("a@b.c"), "a***@b.c");
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_790_468_836);
        assert_eq!(httpdate_rfc3339(at), "2026-09-27T00:27:16Z");
    }
}
