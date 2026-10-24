use crate::usage::UsageSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QueryStatus {
    Latest,
    Expired,
    Failed,
    Disabled,
    Unsupported,
    IdentityUnknown,
    IdentityChanged,
    Backoff,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QueryError {
    Authentication,
    RateLimited,
    Redirect,
    Network,
    InvalidData,
    Timeout,
    Identity,
    Cancelled,
    Persistence,
}
#[derive(Clone, Debug)]
pub struct QueryFailure {
    pub category: QueryError,
    pub retry_after: Option<i64>,
}
impl QueryFailure {
    pub fn cli(message: &str) -> Self {
        let lower = message.to_ascii_lowercase();
        let category = if [
            "401",
            "403",
            "unauthorized",
            "authentication",
            "sign in",
            "login",
        ]
        .iter()
        .any(|s| lower.contains(s))
        {
            QueryError::Authentication
        } else if lower.contains("429") {
            QueryError::RateLimited
        } else if lower.contains("timed out") || lower.contains("timeout") {
            QueryError::Timeout
        } else if lower.contains("invalid")
            || lower.contains("missing")
            || lower.contains("no authenticated")
        {
            QueryError::InvalidData
        } else {
            QueryError::Network
        };
        Self {
            category,
            retry_after: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub id: String,
    pub scope: String,
    pub period: String,
    pub window_minutes: Option<u32>,
    pub used_percent: Option<f64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quantity {
    pub id: String,
    pub unit: String,
    pub currency: Option<String>,
    pub used: Option<String>,
    pub limit: Option<String>,
    pub remaining: Option<String>,
}
#[derive(Clone, Copy, Debug)]
pub enum QuerySource {
    Cli,
    Http,
}
/// Adapter-neutral output: HTTP balances and scoped CLI windows use the same
/// acceptance, identity, persistence and backoff path.
pub struct QueryOutput {
    pub available: Option<bool>,
    pub source: QuerySource,
    pub windows: Vec<QuotaWindow>,
    pub quantities: Vec<Quantity>,
    pub display: Option<UsageSnapshot>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountScope {
    pub provider: String,
    pub backend: String,
    pub endpoint: Option<String>,
    pub account_label: Option<String>,
    pub organization: Option<String>,
    pub workspace: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    pub schema: u32,
    pub scope: AccountScope,
    pub identity_key: String,
    pub identity_generation: u64,
    pub fetched_at: i64,
    pub valid_until: i64,
    pub source: String,
    pub windows: Vec<QuotaWindow>,
    pub quantities: Vec<Quantity>,
    pub display: Option<UsageSnapshot>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub status: QueryStatus,
    pub identity_key: Option<String>,
    pub identity_generation: Option<u64>,
    pub latest_success: Option<AccountSnapshot>,
    pub from_cache: bool,
    pub attempted_at: i64,
    pub next_retry_at: Option<i64>,
    pub error: Option<QueryError>,
}
impl QueryResult {
    pub fn empty(status: QueryStatus, now: i64) -> Self {
        Self {
            status,
            identity_key: None,
            identity_generation: None,
            latest_success: None,
            from_cache: false,
            attempted_at: now,
            next_retry_at: None,
            error: None,
        }
    }
}
pub fn utilization(snapshot: &AccountSnapshot, now: i64) -> Option<f64> {
    if snapshot.schema != 2 || snapshot.valid_until <= now {
        return None;
    }
    // Only account-wide windows may affect account quota protection; model-specific
    // percentages have no universal provider-wide interpretation.
    snapshot
        .windows
        .iter()
        .filter(|w| w.scope == "account" && w.resets_at.map_or(true, |t| t > now))
        .filter_map(|w| w.used_percent)
        .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
        .map(|p| p / 100.0)
        .reduce(f64::max)
}
pub fn validate(snapshot: &AccountSnapshot) -> bool {
    if snapshot.schema != 2
        || snapshot.identity_key.is_empty()
        || snapshot.valid_until <= snapshot.fetched_at
    {
        return false;
    }
    if snapshot.windows.is_empty() && snapshot.quantities.is_empty() {
        return false;
    }
    let mut ids = std::collections::HashSet::new();
    if !snapshot.windows.iter().all(|w| {
        !w.id.is_empty()
            && ids.insert(&w.id)
            && !w.scope.is_empty()
            && w.used_percent
                .map_or(true, |p| p.is_finite() && (0.0..=100.0).contains(&p))
    }) {
        return false;
    }
    let mut quantities = std::collections::HashSet::new();
    snapshot.quantities.iter().all(|q| {
        !q.id.is_empty()
            && quantities.insert(&q.id)
            && !q.unit.is_empty()
            && (q.unit != "currency"
                || q.currency
                    .as_ref()
                    .is_some_and(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase())))
            && [&q.used, &q.limit, &q.remaining]
                .iter()
                .all(|s| s.as_ref().map_or(true, |s| decimal(s)))
    })
}
pub fn decimal(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() <= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCapability {
    /// Named wallet target, not an attestation of CLI session/turn billing.
    pub backend: Option<&'static str>,
    pub method: &'static str,
    pub state: &'static str,
    pub authentication: &'static str,
}
pub fn capability(id: tokage_core::ProviderId) -> QueryCapability {
    use tokage_core::ProviderId::*;
    match id {
        ClaudeCode | CursorCli | CopilotCli | AntigravityCli => QueryCapability {
            backend: None,
            method: "CLI",
            state: "IDENTITY_REQUIRED",
            authentication: "OFFICIAL_SUBSCRIPTION",
        },
        CodexCli => QueryCapability {
            backend: None,
            method: "CLI",
            state: "IDENTITY_PENDING",
            authentication: "OFFICIAL_ACCOUNT",
        },
        Opencode | Reasonix => QueryCapability {
            backend: Some("deepseek"),
            method: "HTTP",
            state: "IDENTITY_REQUIRED",
            authentication: "OFFICIAL_DEEPSEEK",
        },
        KiroCli => QueryCapability {
            backend: None,
            method: "CLI",
            state: "IDENTITY_REQUIRED",
            authentication: "OFFICIAL_ACCOUNT",
        },
        _ => QueryCapability {
            backend: None,
            method: "NONE",
            state: "UNSUPPORTED",
            authentication: "NONE",
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_wallet_never_attests_or_recovers_unrelated_cli_turns() {
        use tokage_core::ProviderId;
        assert_eq!(capability(ProviderId::Opencode).backend, Some("deepseek"));
        assert!(!binds_dispatch(ProviderId::Opencode));
        assert!(binds_dispatch(ProviderId::ClaudeCode));
        assert!(binds_dispatch(ProviderId::CursorCli));
    }
    fn sample() -> AccountSnapshot {
        AccountSnapshot {
            available: None,
            schema: 2,
            scope: AccountScope {
                provider: "CURSOR_CLI".into(),
                backend: "cursor".into(),
                endpoint: None,
                account_label: None,
                organization: None,
                workspace: None,
            },
            identity_key: "opaque".into(),
            identity_generation: 1,
            fetched_at: 10,
            valid_until: 100,
            source: "CLI".into(),
            windows: vec![QuotaWindow {
                id: "a".into(),
                scope: "account".into(),
                period: "rolling".into(),
                window_minutes: Some(300),
                used_percent: Some(1.0),
                resets_at: None,
            }],
            quantities: vec![],
            display: None,
        }
    }
    #[test]
    fn percentage_unknown_scope_and_expiry() {
        let mut s = sample();
        assert_eq!(utilization(&s, 11), Some(0.01));
        s.windows[0].used_percent = Some(100.0);
        assert_eq!(utilization(&s, 11), Some(1.0));
        assert_eq!(utilization(&s, 100), None);
        s.windows[0].resets_at = Some(11);
        assert_eq!(utilization(&s, 11), None);
        s.windows[0].resets_at = None;
        s.windows[0].scope = "model:x".into();
        assert_eq!(utilization(&s, 11), None);
        s.windows[0].used_percent = None;
        assert!(validate(&s));
        s.windows[0].used_percent = Some(f64::NAN);
        assert!(!validate(&s));
    }
    #[test]
    fn distinct_same_period_models_and_decimal_currencies() {
        let mut s = sample();
        let mut w = s.windows[0].clone();
        w.id = "b".into();
        w.scope = "model:b".into();
        s.windows.push(w);
        assert!(validate(&s));
        for currency in ["USD", "CNY"] {
            s.quantities.push(Quantity {
                id: currency.into(),
                unit: "currency".into(),
                currency: Some(currency.into()),
                used: None,
                limit: None,
                remaining: Some("10000000000000000000.0001".into()),
            });
        }
        assert!(validate(&s));
        assert_eq!(
            serde_json::from_str::<AccountSnapshot>(&serde_json::to_string(&s).unwrap())
                .unwrap()
                .quantities
                .len(),
            2
        );
        s.quantities[0].remaining = Some("NaN".into());
        assert!(!validate(&s));
        s.windows[1].id = "a".into();
        assert!(!validate(&s));
    }
}

/// A named wallet cannot attest or recover the credential source of arbitrary CLI turns.
pub fn binds_dispatch(id: tokage_core::ProviderId) -> bool {
    capability(id).backend.is_none()
}
