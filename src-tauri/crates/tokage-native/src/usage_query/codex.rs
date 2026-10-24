//! Pure decoder for the public app-server account/rateLimits/read response.
//! Transport remains disabled until credential-refresh behavior is verified.
use super::contract::{decimal, Quantity, QueryOutput, QuerySource, QuotaWindow};
use serde_json::Value;

pub fn parse(response: &Value) -> Result<QueryOutput, &'static str> {
    let mut out = QueryOutput {
        available: None,
        source: QuerySource::Cli,
        windows: vec![],
        quantities: vec![],
        display: None,
    };
    let buckets: Vec<(String, &Value)> = if let Some(map) = response
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|m| !m.is_empty())
    {
        map.iter()
            .map(|(key, value)| (key.clone(), value))
            .collect()
    } else {
        let legacy = response
            .get("rateLimits")
            .filter(|v| v.is_object())
            .ok_or("Missing Codex rate limits")?;
        vec![(
            legacy
                .get("limitId")
                .and_then(Value::as_str)
                .unwrap_or("legacy")
                .into(),
            legacy,
        )]
    };
    for (id, bucket) in buckets {
        if id.is_empty() || id.len() > 200 {
            return Err("Invalid Codex bucket identifier");
        }
        for slot in ["primary", "secondary"] {
            let Some(window) = bucket.get(slot).filter(|v| !v.is_null()) else {
                continue;
            };
            let used = window
                .get("usedPercent")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
                .ok_or("Invalid Codex usage percentage")?;
            let minutes = match window.get("windowDurationMins").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(
                    v.as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|n| *n > 0)
                        .ok_or("Invalid Codex window duration")?,
                ),
            };
            let reset = match window.get("resetsAt").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(
                    v.as_i64()
                        .filter(|n| *n > 0)
                        .ok_or("Invalid Codex reset time")?,
                ),
            };
            out.windows.push(QuotaWindow {
                id: format!("{id}/{slot}"),
                scope: format!("bucket:{id}"),
                period: slot.into(),
                window_minutes: minutes,
                used_percent: Some(used),
                resets_at: reset,
            });
        }
        if let Some(credit) = bucket.get("credits") {
            // Unlimited is a flag, never an invented numeric balance or denominator.
            if let Some(balance) = credit.get("balance").and_then(Value::as_str) {
                if !decimal(balance) {
                    return Err("Invalid Codex credits balance");
                }
                out.quantities.push(Quantity {
                    id: format!("{id}/credits"),
                    unit: "credits".into(),
                    currency: None,
                    used: None,
                    limit: None,
                    remaining: Some(balance.into()),
                });
            }
        }
    }
    if out.windows.is_empty() && out.quantities.is_empty() {
        return Err("Missing Codex quota quantities");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn multi_bucket_wins_and_unknown_fields_are_ignored() {
        let out = parse(&json!({"rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{
            "codex":{"primary":{"usedPercent":25,"windowDurationMins":300},"credits":{"balance":"10.50","unlimited":false}},
            "spark":{"secondary":{"usedPercent":0,"resetsAt":2000000000},"future":true}
        }})).unwrap();
        assert_eq!(out.windows.len(), 2);
        assert_eq!(out.windows[0].scope, "bucket:codex");
        assert_eq!(out.windows[0].used_percent, Some(25.0));
        assert!(out.windows[1].window_minutes.is_none());
        assert_eq!(out.quantities[0].remaining.as_deref(), Some("10.50"));
        assert!(out.quantities[0].limit.is_none());
    }
    #[test]
    fn missing_is_unknown_and_invalid_is_rejected() {
        assert!(parse(&json!({"rateLimits":{}})).is_err());
        assert!(parse(&json!({"rateLimits":{"primary":{"usedPercent":101}}})).is_err());
        assert!(parse(
            &json!({"rateLimits":{"primary":{"usedPercent":0,"windowDurationMins":-1}}})
        )
        .is_err());
        assert!(parse(&json!({"rateLimits":{"credits":{"unlimited":true}}})).is_err());
        let out =
            parse(&json!({"rateLimits":{"primary":null,"secondary":{"usedPercent":100}}})).unwrap();
        assert_eq!(out.windows.len(), 1);
        assert!(out.windows[0].resets_at.is_none());
        assert!(out.quantities.is_empty());
    }
}
