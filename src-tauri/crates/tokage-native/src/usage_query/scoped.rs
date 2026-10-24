//! CLI-specific quantities and scopes. Never infer account quota from session usage.
use super::contract::{Quantity, QueryOutput, QuerySource, QuotaWindow};
use regex::Regex;
use std::sync::OnceLock;
use tokage_core::ProviderId;

pub(crate) fn parse(provider: ProviderId, text: &str) -> Result<QueryOutput, String> {
    let display = crate::chat::quota::parse_slash_quota_output(provider, text)?;
    let mut output = QueryOutput {
        source: QuerySource::Cli,
        windows: vec![],
        quantities: vec![],
        display: Some(display),
    };
    match provider {
        ProviderId::ClaudeCode => {
            static PERCENT: OnceLock<Regex> = OnceLock::new();
            let percent = PERCENT.get_or_init(|| {
                Regex::new(r"(?i)^[^0-9+\-.]*(\d{1,3}(?:\.\d+)?)%\s*used\b").unwrap()
            });
            let lines: Vec<_> = text.lines().map(str::trim).collect();
            let header = |s: &str| s == "Current session" || s.starts_with("Current week");
            let mut ids = std::collections::HashSet::new();
            for (index, label) in lines.iter().enumerate() {
                if !header(label) {
                    continue;
                }
                let section: Vec<_> = lines[index + 1..]
                    .iter()
                    .take_while(|s| !header(s))
                    .collect();
                let used = section
                    .iter()
                    .find_map(|s| percent.captures(s))
                    .and_then(|c| c[1].parse::<f64>().ok())
                    .filter(|p| (0.0..=100.0).contains(p));
                let Some(used) = used else {
                    continue;
                };
                let (id, scope, period, minutes) = if *label == "Current session" {
                    ("session".to_string(), "account".to_string(), "session", 300)
                } else if *label == "Current week (all models)" || *label == "Current week" {
                    (
                        "week-all".to_string(),
                        "account".to_string(),
                        "weekly",
                        10080,
                    )
                } else {
                    let Some(model) = label
                        .strip_prefix("Current week (")
                        .and_then(|s| s.strip_suffix(')'))
                    else {
                        continue;
                    };
                    if model.is_empty() || model.len() > 100 {
                        continue;
                    }
                    (
                        format!("week-{model}"),
                        format!("model:{model}"),
                        "weekly",
                        10080,
                    )
                };
                if !ids.insert(id.clone()) {
                    return Err("Invalid duplicate Claude usage scope".into());
                }
                let reset = section
                    .iter()
                    .find_map(|s| s.strip_prefix("Resets "))
                    .and_then(|s| {
                        crate::chat::quota::parse_utc_iso_to_unix(
                            &crate::chat::quota::parse_reset_time_to_utc(s),
                        )
                        .ok()
                    });
                output.windows.push(QuotaWindow {
                    id,
                    scope,
                    period: period.into(),
                    window_minutes: Some(minutes),
                    used_percent: Some(used),
                    resets_at: reset,
                });
            }
            // Rebuild the compatibility display from accepted scoped windows only.
            if let Some(display) = output.display.as_mut() {
                display.windows = output
                    .windows
                    .iter()
                    .map(|w| crate::usage::UsageWindow {
                        window_minutes: w.window_minutes.unwrap_or(0),
                        used_percent: w.used_percent.unwrap(),
                        resets_at: w.resets_at,
                    })
                    .collect();
            }
        }
        ProviderId::CopilotCli => {
            let display = output.display.as_ref().unwrap();
            output.windows.push(QuotaWindow {
                id: "plan".into(),
                scope: "account".into(),
                period: "plan".into(),
                window_minutes: None,
                used_percent: display.windows.first().map(|w| w.used_percent),
                resets_at: None,
            });
            static AMOUNT: OnceLock<Regex> = OnceLock::new();
            let amount = AMOUNT.get_or_init(|| Regex::new(r"^\s*([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?)\s*/\s*([0-9]+(?:,[0-9]{3})*(?:\.[0-9]+)?)\s+(AIC|premium requests)\s*$").unwrap());
            // Absolute allocation must be attached to Plan, never the session footer.
            let mut plan = false;
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Plan ") {
                    plan = true;
                    continue;
                }
                if !plan || trimmed.is_empty() {
                    continue;
                }
                if let Some(c) = amount.captures(line) {
                    let used = c[1].replace(',', "");
                    let limit = c[2].replace(',', "");
                    if output.quantities.is_empty() {
                        output.quantities.push(Quantity {
                            id: "plan-allocation".into(),
                            unit: if &c[3] == "AIC" {
                                "credits"
                            } else {
                                "premium_requests"
                            }
                            .into(),
                            currency: None,
                            used: Some(used),
                            limit: Some(limit),
                            remaining: None,
                        });
                    } else {
                        return Err("Invalid duplicate Copilot plan allocation".into());
                    }
                } else {
                    plan = false;
                }
            }
        }
        _ => return Err("Unsupported scoped CLI quota parser".into()),
    }
    if output.windows.is_empty() && output.quantities.is_empty() {
        return Err("Missing recognized CLI quota scope".into());
    }
    Ok(output)
}

/// Preserve the independently reported allocations; only Included is account-wide.
pub(crate) fn cursor(snapshot: crate::usage::UsageSnapshot) -> QueryOutput {
    use crate::usage::UsageRowKind;
    let windows = snapshot
        .details
        .as_ref()
        .map(|details| {
            details
                .rows
                .iter()
                .filter_map(|row| {
                    let (id, scope) = match row.kind {
                        UsageRowKind::Included => ("included", "account"),
                        UsageRowKind::Auto => ("auto", "allocation:auto"),
                        UsageRowKind::Api => ("api", "allocation:api"),
                        UsageRowKind::OnDemand => ("on-demand", "allocation:on-demand"),
                    };
                    row.used_percent.map(|used| QuotaWindow {
                        id: id.into(),
                        scope: scope.into(),
                        period: "plan".into(),
                        window_minutes: None,
                        used_percent: Some(used),
                        resets_at: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    QueryOutput {
        source: QuerySource::Cli,
        windows,
        quantities: vec![],
        display: Some(snapshot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_preserves_allocations_without_summing_or_inventing_reset() {
        use crate::usage::*;
        let out = cursor(UsageSnapshot {
            windows: vec![],
            observed_at: None,
            source: UsageSource::CliOutput,
            details: Some(UsageDetails {
                plan: "Pro".into(),
                reset_label: Some("Resets Nov 1".into()),
                rows: vec![
                    UsageDetailRow {
                        kind: UsageRowKind::Included,
                        used_percent: Some(33.0),
                        value: "33% used".into(),
                    },
                    UsageDetailRow {
                        kind: UsageRowKind::Auto,
                        used_percent: Some(34.0),
                        value: "34% used".into(),
                    },
                    UsageDetailRow {
                        kind: UsageRowKind::Api,
                        used_percent: Some(8.0),
                        value: "8% used".into(),
                    },
                    UsageDetailRow {
                        kind: UsageRowKind::OnDemand,
                        used_percent: None,
                        value: "Disabled".into(),
                    },
                ],
            }),
        });
        assert_eq!(out.windows.len(), 3);
        assert_eq!(
            out.windows.iter().filter(|w| w.scope == "account").count(),
            1
        );
        assert_eq!(out.windows[0].used_percent, Some(33.0));
        assert!(out
            .windows
            .iter()
            .all(|w| w.resets_at.is_none() && w.window_minutes.is_none()));
        assert_eq!(
            out.display.unwrap().details.unwrap().rows[3].value,
            "Disabled"
        );
    }
    #[test]
    fn claude_preserves_model_scope_and_partial_windows() {
        let out = parse(ProviderId::ClaudeCode, "Current session\n25% used\nCurrent week (all models)\nloading\nCurrent week (Sonnet)\n98% used\nResets unknown").unwrap();
        assert_eq!(out.windows.len(), 2);
        assert_eq!(out.windows[0].scope, "account");
        assert_eq!(out.windows[1].scope, "model:Sonnet");
        assert!(out.windows[1].resets_at.is_none());
        assert_eq!(out.display.unwrap().windows.len(), 2);
    }
    #[test]
    fn claude_rejects_unknown_and_duplicate_scope() {
        assert!(parse(ProviderId::ClaudeCode, "Current week mystery\n99% used").is_err());
        assert!(parse(
            ProviderId::ClaudeCode,
            "Current session\n2% used\nCurrent session\n3% used"
        )
        .is_err());
        assert!(parse(ProviderId::ClaudeCode, "Current session\n101% used").is_err());
    }
    #[test]
    fn copilot_keeps_absolute_credits_without_fabricating_period() {
        let out = parse(
            ProviderId::CopilotCli,
            "AI Credits 0 (4s)\nPlan ■■■ 29% used\n 5,900 / 20,000 AIC\nSession 0 / 100 AIC",
        )
        .unwrap();
        assert_eq!(out.quantities.len(), 1);
        assert_eq!(out.quantities[0].used.as_deref(), Some("5900"));
        assert_eq!(out.quantities[0].limit.as_deref(), Some("20000"));
        assert!(out.quantities[0].remaining.is_none());
        assert!(out.windows[0].window_minutes.is_none());
        assert!(out.windows[0].resets_at.is_none());
    }
    #[test]
    fn copilot_never_uses_session_credits_as_account_allocation() {
        let out = parse(
            ProviderId::CopilotCli,
            "Plan 0% used\nSession 0 AIC\n 0 / 100 AIC",
        )
        .unwrap();
        assert!(out.quantities.is_empty());
        assert!(parse(ProviderId::CopilotCli, "Session: 0 AIC used").is_err());
        let out = parse(
            ProviderId::CopilotCli,
            "Plan 50% used\n 150 / 300 premium requests",
        )
        .unwrap();
        assert_eq!(out.quantities[0].unit, "premium_requests");
    }
}
