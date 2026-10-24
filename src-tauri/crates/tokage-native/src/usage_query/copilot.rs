//! Account quota RPC, never a session or ordinary chat prompt.
use super::contract::*;
use crate::{env::ShellEnv, providers};
use serde_json::Value;
use std::path::Path;
fn fail(category: QueryError) -> QueryFailure {
    QueryFailure {
        category,
        retry_after: None,
    }
}
pub(crate) async fn query(exe: &Path, env: &ShellEnv) -> Result<QueryOutput, QueryFailure> {
    let (auth, quota) = providers::copilot_quota_rpc(exe, env)
        .await
        .ok_or_else(|| fail(QueryError::Network))?;
    if auth["isAuthenticated"] != true {
        return Err(fail(QueryError::Authentication));
    }
    parse(&quota.ok_or_else(|| fail(QueryError::InvalidData))?)
}
fn amount(v: &Value) -> Option<String> {
    let s = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    decimal(&s).then_some(s)
}
pub fn parse(value: &Value) -> Result<QueryOutput, QueryFailure> {
    let buckets = value["quotaSnapshots"]
        .as_object()
        .ok_or_else(|| fail(QueryError::InvalidData))?;
    let mut windows = vec![];
    let mut quantities = vec![];
    for (id, bucket) in buckets {
        if id.is_empty() || id.len() > 200 {
            return Err(fail(QueryError::InvalidData));
        }
        let unlimited = bucket["isUnlimitedEntitlement"] == true
            || bucket["entitlementRequests"].as_i64() == Some(-1);
        let unit = if bucket["tokenBasedBilling"] == true {
            "copilot_billing_units"
        } else {
            "requests"
        };
        let used = amount(&bucket["usedRequests"]);
        let limit = (!unlimited)
            .then(|| amount(&bucket["entitlementRequests"]))
            .flatten();
        if used.is_some() || limit.is_some() {
            quantities.push(Quantity {
                id: id.clone(),
                unit: unit.into(),
                currency: None,
                used,
                limit,
                remaining: None,
            });
        }
        if !unlimited && bucket["hasQuota"] != false {
            if let Some(remaining) = bucket["remainingPercentage"]
                .as_f64()
                .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
            {
                // Some CLI versions report a fetch timestamp in resetDate. Never invent a future reset.
                let reset = bucket["resetDate"]
                    .as_str()
                    .and_then(|s| crate::chat::quota::parse_utc_iso_to_unix(s).ok())
                    .filter(|t| *t > super::now() + 60);
                windows.push(QuotaWindow {
                    id: id.clone(),
                    scope: format!("quota:{id}"),
                    period: "plan".into(),
                    window_minutes: None,
                    used_percent: Some(100.0 - remaining),
                    resets_at: reset,
                });
            }
        }
    }
    if windows.is_empty() && quantities.is_empty() {
        return Err(fail(QueryError::InvalidData));
    }
    Ok(QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows,
        quantities,
        display: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_finite_quota_without_fabricating_ai_credit_conversion() {
        let q=parse(&json!({"quotaSnapshots":{"premium_interactions":{"entitlementRequests":20000,"usedRequests":5900,"remainingPercentage":70.5,"tokenBasedBilling":true,"hasQuota":true}}})).unwrap();
        assert_eq!(q.windows[0].used_percent, Some(29.5));
        assert!(q.windows[0].window_minutes.is_none());
        assert!(q.windows[0].resets_at.is_none());
        assert_eq!(q.quantities[0].used.as_deref(), Some("5900"));
        assert_eq!(q.quantities[0].unit, "copilot_billing_units");
    }
    #[test]
    fn unlimited_or_missing_quota_is_not_exhaustion() {
        let q=parse(&json!({"quotaSnapshots":{"chat":{"isUnlimitedEntitlement":true,"entitlementRequests":-1,"usedRequests":0,"remainingPercentage":0}}})).unwrap();
        assert!(q.windows.is_empty());
        assert!(q.quantities[0].limit.is_none());
        assert!(parse(&json!({"quotaSnapshots":{}})).is_err());
    }
}
