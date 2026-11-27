//! The Accounts list fed `jcode usage --json` output in the shapes real
//! accounts produce (recorded, then identity-stripped).
use super::accounts::{AccountsData, build_rows};
use crate::accounts::ResetProvider;
use jcode_sdk::{LoginMethod, LoginProvider};

const AUTH: &str = r#"{"providers":[
    {"id":"claude","status":"available","display_name":"Anthropic/Claude","auth_kind":"OAuth"},
    {"id":"openai","status":"available","display_name":"OpenAI","auth_kind":"OAuth"}]}"#;

/// Four logins: a Max login with one of two saved grants spent and no limit
/// reached (recorded from a real account), an organization seat with saved
/// grants, an enterprise credits seat and a business ChatGPT seat holding
/// credits whose usage cost the CLI cannot price.
const USAGE: &str = r#"{"providers":[
  {"provider_name":"Anthropic (Claude) (x***g@example.com)",
   "limits":[{"name":"7-day window","usage_percent":28.0,"reset_in":"3d 12h"}],
   "extra_info":[["Plan","max_20x"],
     ["Today","294,467 in · 146 out · $0.77 est."],
     ["Lifetime","9.978 Bil in · $3796.16 est. + unknown cost (65 unpriced) · since 2026-11-15"],
     ["Account label","claude-otter"]],
   "banked_reset":{"provider":"claude","account_label":"claude-otter","available_count":1,"usable_count":1,
     "expires_at":"2027-03-09T07:59:00+00:00","session_reset_available":false,"limit_reached":false,
     "summary":"✦ 1 reset · expires 101d21h","next_available_at":null},
   "account":{"provider":"claude","label":"claude-otter","email":"x***g@example.com","plan":"Max 20x","display_name":"x***g@example.com · Max 20x","kind":"subscription","active":false},
   "error":null},
  {"provider_name":"Anthropic (Claude) (v***o@example.com)",
   "limits":[{"name":"5-hour window","usage_percent":34.0,"reset_in":"3h 40m"},{"name":"7-day window","usage_percent":6.0,"reset_in":"6d 20h"}],
   "extra_info":[["Account label","claude-fox"],["Today","1 in · $6.28 est."],["Lifetime","2 in · $16.38 est."]],
   "banked_reset":{"provider":"claude","account_label":"claude-fox","available_count":3,"usable_count":1,
     "expires_at":"2026-12-10T22:00:00+00:00","session_reset_available":false,"limit_reached":false},
   "account":{"provider":"claude","label":"claude-fox","plan":"Max 20x","display_name":"AI CoE 2 · Max 20x","kind":"subscription","active":true},
   "error":null},
  {"provider_name":"Anthropic (Claude) (claude-panda)",
   "limits":[],
   "extra_info":[["Account label","claude-panda"],["Today","No recorded usage"],["Lifetime","No recorded usage"]],
   "banked_reset":{"provider":"claude","account_label":"claude-panda","available_count":0,"usable_count":0,
     "session_reset_available":false,"limit_reached":false},
   "account":{"provider":"claude","label":"claude-panda","display_name":"ent · Enterprise","kind":"credits","active":false},
   "credits":{"state":"known","unit":"usd","balance":194.13,"used":105.87,"limit":300.0,"unlimited":false,"summary":"$194.13 left · $105.87 of $300.00 used"},
   "error":null},
  {"provider_name":"OpenAI (ChatGPT) (v***o@example.com)",
   "limits":[{"name":"7-day window","usage_percent":10.0,"reset_in":"4d"}],
   "extra_info":[["Plan","business"],["Account label","openai-otter"],
     ["Today","109,441 in · 729 out · cost unknown"],["Lifetime","11.498 Bil in · cost unknown"]],
   "banked_reset":{"provider":"openai","account_label":"openai-otter","available_count":0,"limit_reached":false},
   "account":{"provider":"openai","label":"openai-otter","plan":"Business","display_name":"v***o@example.com · Business","kind":"credits","active":true},
   "credits":{"state":"known","unit":"credits","balance":45001.5376165,"unlimited":false,"summary":"45001.54 credits left"},
   "error":null}]}"#;

fn rows() -> Vec<super::accounts::AccountRow> {
    let provider = |id, display_name| LoginProvider {
        id,
        display_name,
        detail: "",
        method: LoginMethod::OAuth,
    };
    let mut accounts = crate::accounts::parse(AUTH).unwrap();
    crate::accounts::merge_usage_for_tests(&mut accounts, USAGE);
    let login = |provider, label: &str, active| jcode_base::auth::account_pool::OAuthLogin {
        provider,
        label: label.into(),
        email: None,
        active,
    };
    let data = AccountsData {
        accounts: Some(accounts),
        logins: vec![
            login("claude", "claude-otter", false),
            login("claude", "claude-fox", true),
            login("claude", "claude-panda", false),
            login("openai", "openai-otter", true),
        ],
        ..Default::default()
    };
    build_rows(
        &[
            provider("claude", "Anthropic/Claude"),
            provider("openai", "OpenAI"),
        ],
        None,
        false,
        &|_| false,
        &data,
    )
}

#[test]
fn each_login_shows_its_own_balance_and_leaves_out_what_the_provider_cannot_price() {
    let rows = rows();
    let usage = |label: &str| {
        rows.iter()
            .find(|row| row.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("{label}"))
            .usage
            .clone()
    };
    // The credits seat shows its balance, not "today n/a · lifetime n/a".
    assert_eq!(
        usage("claude-panda").as_deref(),
        Some("$194.13 left · $105.87 of $300.00 used · today $0.00 · lifetime $0.00 (est.)")
    );
    assert_eq!(
        usage("openai-otter").as_deref(),
        Some("45001.54 credits left")
    );
    // A login without a balance keeps only the spend it can state.
    assert_eq!(
        usage("claude-fox").as_deref(),
        Some("today $6.28 · lifetime $16.38 (est.)")
    );
}

#[test]
fn only_a_login_with_a_usable_saved_reset_offers_one() {
    let rows = rows();
    let usable = |label: &str| {
        rows.iter()
            .find(|row| row.label.as_deref() == Some(label))
            .and_then(|row| row.banked.as_ref())
            .map_or(0, |reset| reset.usable())
    };
    // One of two grants is spent: the account reports one saved, one usable.
    assert_eq!(usable("claude-otter"), 1);
    // Three saved but only one reported usable: the click spends one.
    assert_eq!(usable("claude-fox"), 1);
    assert_eq!(usable("claude-panda"), 0);
    assert_eq!(usable("openai-otter"), 0);
    let otter = rows[0].banked.as_ref().unwrap();
    assert_eq!(otter.provider, ResetProvider::Claude);
}
