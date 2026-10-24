//! Structured zero-turn management response; groups never become account-wide quota.
use super::contract::{QueryOutput, QuerySource, QuotaWindow};
use crate::usage::{UsageSnapshot, UsageSource, UsageWindow};
use serde_json::Value;

pub fn parse(text: &str) -> Result<QueryOutput, &'static str> {
    let v: Value = serde_json::from_str(text).map_err(|_| "Invalid quota JSON")?;
    if v["status"] != "SUCCESS"
        || v["command"]["name"] != "usage"
        || v["num_turns"].as_u64() != Some(0)
    {
        return Err("Not a successful zero-turn usage response");
    }
    let groups = v["command"]["data"]["groups"]
        .as_array()
        .ok_or("Missing quota groups")?;
    let mut windows = vec![];
    let mut ids = std::collections::HashSet::new();
    for (group_index, group) in groups.iter().enumerate() {
        let name = group["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 200);
        for (index, bucket) in group["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(remaining) = bucket["remaining_fraction"]
                .as_f64()
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            else {
                continue;
            };
            let Some(period) = bucket["window"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 100)
            else {
                continue;
            };
            let bucket_id = bucket["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 200)
                .map(str::to_string)
                .unwrap_or_else(|| format!("position:{index}"));
            // JSON tuple avoids separator collisions in names and identifiers.
            let group_id = name
                .map(str::to_string)
                .unwrap_or_else(|| format!("position:{group_index}"));
            let id = serde_json::json!([group_id, bucket_id, period]).to_string();
            if !ids.insert(id.clone()) {
                return Err("Invalid duplicate quota bucket");
            }
            windows.push(QuotaWindow {
                id,
                scope: name
                    .map(|n| format!("group:{n}"))
                    .unwrap_or_else(|| "unclassified".into()),
                period: period.into(),
                window_minutes: match period {
                    "5h" => Some(300),
                    "weekly" => Some(10080),
                    _ => None,
                },
                used_percent: Some((1.0 - remaining) * 100.0),
                resets_at: bucket["reset_time"].as_str().and_then(|s| reset_time(s)),
            });
        }
    }
    if windows.is_empty() {
        return Err("Missing valid quota windows");
    }
    let display = UsageSnapshot {
        windows: windows
            .iter()
            .map(|w| UsageWindow {
                window_minutes: w.window_minutes.unwrap_or(0),
                used_percent: w.used_percent.unwrap(),
                resets_at: w.resets_at,
            })
            .collect(),
        observed_at: None,
        details: None,
        source: UsageSource::CliOutput,
    };
    Ok(QueryOutput {
        source: QuerySource::Cli,
        windows,
        quantities: vec![],
        display: Some(display),
    })
}

fn reset_time(s: &str) -> Option<i64> {
    if s.len() != 20
        || !s.is_ascii()
        || &s[4..5] != "-"
        || &s[7..8] != "-"
        || &s[10..11] != "T"
        || &s[13..14] != ":"
        || &s[16..17] != ":"
        || &s[19..] != "Z"
    {
        return None;
    }
    for (start, end, min, max) in [
        (0, 4, 0, 9999),
        (5, 7, 1, 12),
        (8, 10, 1, 31),
        (11, 13, 0, 23),
        (14, 16, 0, 59),
        (17, 19, 0, 59),
    ] {
        let n = s[start..end].parse::<u32>().ok()?;
        if n < min || n > max {
            return None;
        }
    }
    let timestamp = crate::chat::quota::parse_utc_iso_to_unix(s).ok()?;
    (crate::chat::quota::format_unix_to_utc_iso(timestamp) == s).then_some(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn response(groups: Value) -> String {
        json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":groups}}}).to_string()
    }
    #[test]
    fn keeps_same_period_groups_and_unknown_period_without_invention() {
        let out = parse(&response(json!([
            {"name":"model-a","buckets":[{"id":"one","window":"5h","remaining_fraction":1.0}]},
            {"name":"model-b","buckets":[{"id":"one","window":"5h","remaining_fraction":0.0},{"id":"one","window":"future","remaining_fraction":0.25}]}
        ]))).unwrap();
        assert_eq!(out.windows.len(), 3);
        assert_ne!(out.windows[0].id, out.windows[1].id);
        assert_eq!(out.windows[0].used_percent, Some(0.0));
        assert_eq!(out.windows[1].used_percent, Some(100.0));
        assert_eq!(out.windows[2].used_percent, Some(75.0));
        assert!(out.windows[2].window_minutes.is_none());
        assert_eq!(out.windows[2].period, "future");
        assert!(out
            .windows
            .iter()
            .all(|w| w.scope != "account" && w.resets_at.is_none()));
    }
    #[test]
    fn reset_dates_are_real_calendar_dates_or_unknown() {
        assert!(reset_time("2026-10-24T00:00:00Z").is_some());
        for raw in [
            "2026-99-24T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-10-24T24:00:00Z",
            "unknown",
        ] {
            assert!(reset_time(raw).is_none());
        }
    }
    #[test]
    fn invalid_and_missing_buckets_do_not_erase_valid_sibling() {
        let out = parse(&response(json!([{"buckets":[{"window":"weekly","remaining_fraction":1.1},{"window":"weekly"},{"window":"weekly","remaining_fraction":0.5}]}]))).unwrap();
        assert_eq!(out.windows.len(), 1);
        assert_eq!(out.windows[0].scope, "unclassified");
    }
    #[test]
    fn rejects_model_turn_error_and_duplicate_buckets() {
        for bad in [
            json!({"status":"ERROR","error":"private credential message"}),
            json!({"status":"SUCCESS","num_turns":1,"command":{"name":"usage"}}),
            json!({"status":"SUCCESS","num_turns":0,"command":{"name":"chat"}}),
        ] {
            let err = parse(&bad.to_string()).err().unwrap();
            assert!(!err.contains("private"));
        }
        assert!(parse(&response(json!([{"name":"a","buckets":[{"id":"x","window":"5h","remaining_fraction":1.0},{"id":"x","window":"5h","remaining_fraction":0.0}]}]))).is_err());
    }
}
