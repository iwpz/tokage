//! Unified, read-only receipt view. Never writes usage or settlement rows.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokage_core::{TurnUsage, UsageSource};
use tokage_runtime::{
    accounting::{HumanEffort, Receipt},
    Snapshot,
};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}
impl Tokens {
    fn add(&mut self, u: &TurnUsage) {
        self.input += u.input_tokens;
        self.cache_read += u.cache_read_tokens;
        self.cache_write += u.cache_write_tokens;
        self.output += u.output_tokens;
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub receipts: u32,
    pub reported_usd: Option<f64>,
    pub known_reported_usd: f64,
    pub unknown_usd_receipts: u32,
    pub estimated_usd: Option<f64>,
    pub known_estimated_usd: f64,
    pub unknown_estimate_receipts: u32,
    pub reported_tokens: Option<Tokens>,
    pub estimated_tokens: Option<Tokens>,
    pub unknown_token_receipts: u32,
}
impl Totals {
    fn add(&mut self, r: &Receipt) {
        self.receipts += 1;
        if let Some(v) = r.reported_usd {
            self.known_reported_usd += v;
        } else {
            self.unknown_usd_receipts += 1;
        }
        if let Some(v) = r.estimated_usd {
            self.known_estimated_usd += v;
        } else {
            self.unknown_estimate_receipts += 1;
        }
        if let Some(u) = &r.attributed_usage {
            if u.source == UsageSource::Estimated {
                self.estimated_tokens
                    .get_or_insert_with(Default::default)
                    .add(u);
            } else {
                self.reported_tokens
                    .get_or_insert_with(Default::default)
                    .add(u);
            }
        } else {
            self.unknown_token_receipts += 1;
        }
        self.reported_usd = if self.unknown_usd_receipts == 0 {
            Some(self.known_reported_usd)
        } else {
            None
        };
        self.estimated_usd = if self.unknown_estimate_receipts == 0 {
            Some(self.known_estimated_usd)
        } else {
            None
        };
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub id: i64,
    pub kind: String,
    pub reference: Option<String>,
    pub session_id: Option<String>,
    pub plan_id: Option<i64>,
    pub status: String,
    pub baseline_usd: f64,
    pub actual_usd: Option<f64>,
    pub delta_usd: Option<f64>,
    pub detail_json: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accounting {
    pub totals: Totals,
    pub phases: BTreeMap<String, Totals>,
    pub receipts: Vec<Receipt>,
    pub context_actions: Vec<tokage_runtime::context_audit::Action>,
    pub unknown_date_receipts: u32,
    pub legacy_unkeyed_receipts: u32,
    pub comparisons: Vec<Comparison>,
    pub comparable_count: u32,
    pub human_effort: Vec<HumanEffort>,
    pub known_human_seconds: u64,
    pub unknown_human_effort: u32,
    pub captured_at: u64,
}
fn valid(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite() && *v >= 0.)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn nonnegative(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let v: i64 = r.get(index)?;
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, v))
}
fn legacy(conn: &Connection, since: u64) -> Result<Vec<Receipt>, String> {
    let mut q=conn.prepare("SELECT u.id,k.event_key,u.attempt_id,a.main_session_id,u.provider_name,COALESCE(u.model_id,'unknown'),u.phase,u.source,u.input_tokens,u.cache_read_tokens,u.cache_write_tokens,u.output_tokens,u.api_calls,u.context_tokens,u.cost_usd,u.equiv_cost_usd,u.credits,CAST(strftime('%s',u.created_at) AS INTEGER) FROM usage_events u LEFT JOIN usage_receipts k ON k.usage_id=u.id LEFT JOIN attempts a ON a.id=u.attempt_id WHERE (CAST(strftime('%s',u.created_at) AS INTEGER)>=?1 OR strftime('%s',u.created_at) IS NULL) AND COALESCE(a.debug_output,'')!='mock'").map_err(err)?;
    let rows = q
        .query_map([i64::try_from(since).map_err(err)?], |r| {
            let id: i64 = r.get(0)?;
            let key: Option<String> = r.get(1)?;
            let source: String = r.get(7)?;
            let usage_source = if source == "ESTIMATED" {
                UsageSource::Estimated
            } else if source == "STDOUT_REGEX" {
                UsageSource::StdoutRegex
            } else {
                UsageSource::Structured
            };
            let cost: Option<f64> = r.get(14)?;
            let equiv: Option<f64> = r.get(15)?;
            let u = TurnUsage {
                input_tokens: nonnegative(r, 8)?,
                cache_read_tokens: nonnegative(r, 9)?,
                cache_write_tokens: nonnegative(r, 10)?,
                output_tokens: nonnegative(r, 11)?,
                api_calls: r.get(12)?,
                context_tokens: r
                    .get::<_, Option<i64>>(13)?
                    .map(|v| {
                        u64::try_from(v)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(13, v))
                    })
                    .transpose()?,
                cost_usd: cost,
                credits: r.get(16)?,
                premium_requests: None,
                source: usage_source,
            };
            let provider:String=r.get(4)?;
            let codex=provider=="CODEX_CLI";
            Ok(Receipt {
                attributed_usage: if codex && usage_source!=UsageSource::Estimated {None}else{Some(u.clone())},
                attribution_basis: if codex {"LEGACY_CODEX_SCOPE_UNKNOWN"}else{"LEGACY_STORED_TURN_RECEIPT"}.into(),
                baseline_turn:None,
                key: key
                    .as_ref()
                    .map(|k| format!("turn:{k}"))
                    .unwrap_or_else(|| format!("legacy_usage:{id}")),
                origin: if key.is_some() {
                    "APP_RECEIPT"
                } else {
                    "LEGACY_UNKEYED"
                }
                .into(),
                run_id: r.get(3)?,
                turn_id: key.clone(),
                task_id: None,
                provider,
                model: r.get(5)?,
                model_basis: "STORED_USAGE_IDENTITY".into(),
                phase: r.get(6)?,
                status: "RECORDED".into(),
                observed_at: r
                    .get::<_, Option<i64>>(17)?
                    .and_then(|v| u64::try_from(v).ok()),
                reported_usd: if usage_source == UsageSource::Estimated {
                    None
                } else {
                    valid(cost)
                },
                estimated_usd: if codex {if usage_source==UsageSource::Estimated {valid(cost)}else{None}}else{valid(equiv).or_else(|| {
                    if usage_source == UsageSource::Estimated {
                        valid(cost)
                    } else {
                        None
                    }
                })},
                estimate_basis: if codex {"LEGACY_CODEX_SCOPE_UNKNOWN"}else{"STORED_EQUIVALENT_OR_CLI_ESTIMATE_NOT_BILLING"}.into(),
                usage: Some(u),
                source,
                price_basis: None,
                evidence_digest: format!(
                    "usage_events:{id};event_key:{}",
                    key.as_deref().unwrap_or("unknown")
                ),
                duplicate_sources: vec![],
            })
        })
        .map_err(err)?;
    rows.map(|v| v.map_err(err)).collect()
}
fn merge(rows: Vec<Receipt>) -> Result<Vec<Receipt>, String> {
    // Validate a bound over all rows before computing any subset. This also
    // bounds sums across token dimensions and across duplicate rows.
    let mut tokens = 0u64;
    let mut dollars = 0f64;
    if rows.len() > u32::MAX as usize {
        return Err("Receipt count overflow".into());
    }
    for r in &rows {
        for u in r.usage.iter().chain(r.attributed_usage.iter()) {
            for n in [
                u.input_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens,
                u.output_tokens,
            ] {
                tokens = tokens.checked_add(n).ok_or("Token aggregate overflow")?;
            }
            for v in [u.cost_usd, u.credits, u.premium_requests]
                .into_iter()
                .flatten()
            {
                if !v.is_finite() || v < 0. {
                    return Err("Invalid native accounting value".into());
                }
            }
        }
        for v in [r.reported_usd, r.estimated_usd].into_iter().flatten() {
            if !v.is_finite() || v < 0. {
                return Err("Invalid accounting value".into());
            }
            dollars += v;
            if !dollars.is_finite() {
                return Err("Money aggregate overflow".into());
            }
        }
    }
    let mut unique = BTreeMap::<String, Receipt>::new();
    for r in rows {
        if let Some(prior) = unique.get_mut(&r.key) {
            let same_tokens = match (&prior.usage, &r.usage) {
                (Some(a), Some(b)) => {
                    (
                        a.input_tokens,
                        a.cache_read_tokens,
                        a.cache_write_tokens,
                        a.output_tokens,
                    ) == (
                        b.input_tokens,
                        b.cache_read_tokens,
                        b.cache_write_tokens,
                        b.output_tokens,
                    ) && a.source == b.source
                        && !a
                            .credits
                            .zip(b.credits)
                            .is_some_and(|(a, b)| (a - b).abs() > 1e-9)
                        && !a
                            .premium_requests
                            .zip(b.premium_requests)
                            .is_some_and(|(a, b)| (a - b).abs() > 1e-9)
                }
                _ => true,
            };
            if prior.origin=="RUNTIME" && r.origin=="RUNTIME" && (prior.attributed_usage!=r.attributed_usage || prior.attribution_basis!=r.attribution_basis || prior.baseline_turn!=r.baseline_turn) {
                return Err(format!("Conflicting usage attribution: {}",r.key));
            }
            if prior.provider != r.provider
                || prior.model != r.model
                || !same_tokens
                || prior
                    .reported_usd
                    .zip(r.reported_usd)
                    .is_some_and(|(a, b)| (a - b).abs() > 1e-9)
            {
                return Err(format!("Conflicting accounting receipt: {}", r.key));
            }
            prior.duplicate_sources.push(r.evidence_digest.clone());
            prior.reported_usd = prior.reported_usd.or(r.reported_usd);
            match (&mut prior.usage, r.usage) {
                (Some(a), Some(b)) => {
                    a.credits = a.credits.or(b.credits);
                    a.premium_requests = a.premium_requests.or(b.premium_requests);
                }
                (slot @ None, value) => *slot = value,
                _ => {}
            }
        } else {
            unique.insert(r.key.clone(), r);
        }
    }
    Ok(unique.into_values().collect())
}
fn bucket(conn: &Connection, t: u64, format: &str) -> Result<String, String> {
    conn.query_row(
        "SELECT strftime(?1,?2,'unixepoch')",
        rusqlite::params![format, i64::try_from(t).map_err(err)?],
        |r| r.get(0),
    )
    .map_err(err)
}
fn comparisons(conn: &Connection, range: &str) -> Result<Vec<Comparison>, String> {
    tokage_storage::savings::list_savings_events(conn, Some(range))
        .map_err(err)?
        .into_iter()
        .map(|s| {
            if !s.baseline_usd.is_finite() || s.actual_usd.is_some_and(|v| !v.is_finite()) {
                return Err("Nonfinite comparison value".into());
            }
            let detail =
                serde_json::from_str::<serde_json::Value>(&s.detail_json).unwrap_or_default();
            let matched = s.kind == "CONTEXT"
                && detail["basis"] == "first_turn_counterfactual_v1"
                && detail["horizonTurns"] == 1;
            let valid_values =
                valid(Some(s.baseline_usd)).is_some() && valid(s.actual_usd).is_some();
            let status = if !s.settled || s.actual_usd.is_none() {
                "UNSETTLED"
            } else if !valid_values {
                "INVALID_VALUES"
            } else if !matched {
                "LEGACY_BASIS_UNVERIFIED"
            } else {
                "COUNTERFACTUAL_NOT_MEASURED_SAVING"
            };
            Ok(Comparison {
                id: s.id.unwrap_or(0),
                kind: s.kind,
                reference: s.ref_id,
                session_id: s.main_session_id,
                plan_id: s.plan_id,
                status: status.into(),
                baseline_usd: s.baseline_usd,
                actual_usd: s.actual_usd,
                delta_usd: if matched && s.settled && valid_values {
                    Some(s.baseline_usd - s.actual_usd.unwrap())
                } else {
                    None
                },
                detail_json: s.detail_json,
            })
        })
        .collect()
}
pub fn apply(
    view: &mut crate::dashboard::UsageDashboard,
    conn: &Connection,
    snapshots: &[Snapshot],
    now: u64,
) -> Result<(), String> {
    let mut rows = Vec::new();
    let mut effort = Vec::new();
    let mut actions=Vec::new();
    for s in snapshots {
        let p = tokage_runtime::accounting::project(s).map_err(err)?;
        rows.extend(p.receipts);
        effort.extend(p.human_effort);
        actions.extend(p.context_actions);
    }
    rows.extend(legacy(conn, now.saturating_sub(365 * 86400))?);
    let rows = merge(rows)?;
    let since = now.saturating_sub(match view.range.as_str() {
        "today" => now % 86400,
        "30d" => 30 * 86400,
        _ => 7 * 86400,
    });
    let in_range = |r: &Receipt| r.observed_at.is_some_and(|t| t >= since && t <= now);
    let mut totals = Totals::default();
    let mut phases = BTreeMap::<String, Totals>::new();
    let mut models = BTreeMap::<(String, String), Totals>::new();
    let mut providers = BTreeMap::<String, Totals>::new();
    let mut series = BTreeMap::<(String, String), Tokens>::new();
    let mut daily = BTreeMap::<String, BTreeMap<(String, String), Totals>>::new();
    let mut dates = BTreeMap::new();
    for r in &rows {
        if in_range(r) {
            totals.add(r);
            phases.entry(r.phase.clone()).or_default().add(r);
            models
                .entry((r.provider.clone(), r.model.clone()))
                .or_default()
                .add(r);
            providers.entry(r.provider.clone()).or_default().add(r);
        }
        if let Some(t) = r
            .observed_at
            .filter(|t| *t <= now && *t >= now.saturating_sub(365 * 86400))
        {
            let date = dates
                .entry(t / 86400)
                .or_insert(bucket(conn, t, "%Y-%m-%d")?);
            daily
                .entry(date.clone())
                .or_default()
                .entry((r.provider.clone(), r.model.clone()))
                .or_default()
                .add(r);
            if in_range(r) {
                if let Some(u) = r
                    .attributed_usage
                    .as_ref()
                    .filter(|u| u.source != UsageSource::Estimated)
                {
                    let key = if view.range == "today" {
                        bucket(conn, t, "%H:00")?
                    } else {
                        date.clone()
                    };
                    series.entry((key, r.provider.clone())).or_default().add(u);
                }
            }
        }
    }
    let unknown_date_receipts = rows.iter().filter(|r| r.observed_at.is_none()).count() as u32;
    if unknown_date_receipts > 0 {
        totals.reported_usd = None;
        totals.estimated_usd = None;
        for r in rows.iter().filter(|r| r.observed_at.is_none()) {
            let phase = phases.entry(r.phase.clone()).or_default();
            phase.reported_usd = None;
            phase.estimated_usd = None;
        }
    }
    // Metadata remains independent of cost receipts; add providers absent from the old fixed list.
    for provider in providers.keys() {
        if !view.providers.iter().any(|p| &p.provider == provider) {
            view.providers
                .push(crate::dashboard::DashboardProviderInfo {
                    provider: provider.clone(),
                    billing: "UNKNOWN".into(),
                    is_gateway: false,
                    custom_endpoint: None,
                    usage_pct: None,
                    usage_pct_source: None,
                    reset_at: None,
                    spent_usd: None,
                    equiv_cost_usd: None,
                    cache_hit_rate: 0.,
                    status: "UNKNOWN".into(),
                    melt_reason: None,
                    models: vec![],
                });
        }
    }
    for p in &mut view.providers {
        let sum = providers.get(&p.provider).cloned().unwrap_or_default();
        p.spent_usd = sum.reported_usd;
        p.equiv_cost_usd = sum.estimated_usd;
        if rows
            .iter()
            .any(|r| r.observed_at.is_none() && r.provider == p.provider)
        {
            p.spent_usd = None;
            p.equiv_cost_usd = None;
        }
        p.models = models
            .iter()
            .filter(|((provider, _), _)| provider == &p.provider)
            .map(|((_, model), s)| {
                let t = s.reported_tokens.clone().unwrap_or_default();
                crate::dashboard::DashboardModelUsage {
                    model_id: model.clone(),
                    input_tokens: t.input,
                    cache_read_tokens: t.cache_read,
                    cache_write_tokens: t.cache_write,
                    output_tokens: t.output,
                    cost_usd: if rows.iter().any(|r| {
                        r.observed_at.is_none() && r.provider == p.provider && r.model == *model
                    }) {
                        None
                    } else {
                        s.reported_usd
                    },
                    equiv_cost_usd: if rows.iter().any(|r| {
                        r.observed_at.is_none() && r.provider == p.provider && r.model == *model
                    }) {
                        None
                    } else {
                        s.estimated_usd
                    },
                    calls: s.receipts,
                }
            })
            .collect();
        p.cache_hit_rate = sum
            .reported_tokens
            .map(|t| {
                let all = t.input + t.cache_read + t.cache_write;
                if all == 0 {
                    0.
                } else {
                    t.cache_read as f64 / all as f64
                }
            })
            .unwrap_or(0.);
    }
    view.token_series = series
        .into_iter()
        .map(
            |((bucket, provider), t)| crate::dashboard::DashboardTokenSeriesPoint {
                bucket,
                provider,
                input: t.input,
                cache_read: t.cache_read,
                cache_write: t.cache_write,
                output: t.output,
            },
        )
        .collect();
    view.daily_usage = daily
        .into_iter()
        .map(|(date, groups)| {
            let mut summary = Totals::default();
            let mut model_rows = Vec::new();
            for ((provider, model), s) in groups {
                let t = s.reported_tokens.clone().unwrap_or_default();
                model_rows.push(crate::dashboard::DailyModelUsagePoint {
                    model_id: model,
                    provider,
                    total_tokens: t.input + t.cache_read + t.cache_write + t.output,
                    input_tokens: t.input,
                    cache_read_tokens: t.cache_read,
                    cache_write_tokens: t.cache_write,
                    output_tokens: t.output,
                    cost_usd: s.reported_usd,
                    equiv_cost_usd: s.estimated_usd,
                });
                summary.receipts += s.receipts;
                summary.known_reported_usd += s.known_reported_usd;
                summary.known_estimated_usd += s.known_estimated_usd;
                summary.unknown_usd_receipts += s.unknown_usd_receipts;
                summary.unknown_estimate_receipts += s.unknown_estimate_receipts;
            }
            let t = model_rows.iter().fold(Tokens::default(), |mut t, m| {
                t.input += m.input_tokens;
                t.cache_read += m.cache_read_tokens;
                t.cache_write += m.cache_write_tokens;
                t.output += m.output_tokens;
                t
            });
            crate::dashboard::DailyUsagePoint {
                date,
                total_tokens: t.input + t.cache_read + t.cache_write + t.output,
                input_tokens: t.input,
                cache_read_tokens: t.cache_read,
                cache_write_tokens: t.cache_write,
                output_tokens: t.output,
                cost_usd: if summary.unknown_usd_receipts == 0 {
                    Some(summary.known_reported_usd)
                } else {
                    None
                },
                equiv_cost_usd: if summary.unknown_estimate_receipts == 0 {
                    Some(summary.known_estimated_usd)
                } else {
                    None
                },
                models: model_rows,
            }
        })
        .collect();
    view.cost_split = crate::dashboard::DashboardCostSplit {
        planning_usd: phases.get("PLANNING").and_then(|p| p.reported_usd),
        execution_usd: phases.get("EXECUTION").and_then(|p| p.reported_usd),
        repair_usd: phases.get("REPAIR").and_then(|p| p.reported_usd),
    };
    // Scopes may overlap across kinds. Never add counterfactual differences into saved cash.
    view.savings.total_usd = None;
    for v in view.savings.by_kind.values_mut() {
        *v = None;
    }
    let comps = comparisons(conn, &view.range)?;
    let comparable_count = comps.iter().filter(|c| c.delta_usd.is_some()).count() as u32;
    effort.retain(|h| {
        h.recorded_at.is_none() || h.recorded_at.is_some_and(|t| t >= since && t <= now)
    });
    let mut known_human_seconds = 0u64;
    for h in effort.iter().filter(|h| h.recorded_at.is_some()) {
        known_human_seconds = known_human_seconds
            .checked_add(h.reported_seconds.unwrap_or(0))
            .ok_or("Human effort overflow")?;
    }
    let unknown_human_effort = effort
        .iter()
        .filter(|h| h.reported_seconds.is_none() || h.recorded_at.is_none())
        .count() as u32;
    let legacy_unkeyed_receipts = rows
        .iter()
        .filter(|r| in_range(r) && r.origin == "LEGACY_UNKEYED")
        .count() as u32;
    view.accounting = Some(Accounting {
        totals,
        phases,
        context_actions: actions.into_iter().filter(|a|a.occurred_at.is_none_or(|t|t>=since&&t<=now)).collect(),
        receipts: rows
            .into_iter()
            .filter(|r| in_range(r) || r.observed_at.is_none())
            .collect(),
        unknown_date_receipts,
        legacy_unkeyed_receipts,
        comparisons: comps,
        comparable_count,
        human_effort: effort,
        known_human_seconds,
        unknown_human_effort,
        captured_at: now,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> Receipt {
        Receipt {
            attributed_usage: None,
            attribution_basis:"TEST_FIXTURE".into(),
            baseline_turn:None,
            key: "turn:one".into(),
            origin: "RUNTIME".into(),
            run_id: Some("run".into()),
            turn_id: Some("one".into()),
            task_id: Some("T1".into()),
            provider: "p".into(),
            model: "m".into(),
            model_basis: "CLI_OBSERVED".into(),
            phase: "REVIEW".into(),
            status: "Completed".into(),
            observed_at: Some(1),
            reported_usd: Some(0.),
            estimated_usd: None,
            estimate_basis: "UNKNOWN".into(),
            usage: Some(TurnUsage {
                input_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                output_tokens: 0,
                api_calls: None,
                context_tokens: None,
                cost_usd: None,
                premium_requests: None,
                credits: None,
                source: UsageSource::Structured,
            }),
            source: "STRUCTURED".into(),
            price_basis: None,
            evidence_digest: "runtime".into(),
            duplicate_sources: vec![],
        }
    }
    #[test]
    fn duplicate_keys_merge_once_and_conflicts_fail_closed() {
        let r = receipt();
        let mut copy = r.clone();
        copy.origin = "APP_RECEIPT".into();
        copy.evidence_digest = "legacy".into();
        let merged = merge(vec![r.clone(), copy.clone()]).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].duplicate_sources, vec!["legacy"]);
        copy.reported_usd = Some(1.);
        assert!(merge(vec![r.clone(), copy]).is_err());
        let mut copy = r.clone();
        copy.usage.as_mut().unwrap().credits = Some(2.);
        let mut first = r.clone();
        first.usage.as_mut().unwrap().credits = Some(1.);
        assert!(merge(vec![first, copy]).is_err());
        let mut copy = r.clone();
        copy.usage.as_mut().unwrap().input_tokens = u64::MAX;
        copy.usage.as_mut().unwrap().output_tokens = 1;
        assert!(merge(vec![copy]).is_err());
        let mut copy = r.clone();
        copy.reported_usd = Some(f64::MAX);
        copy.estimated_usd = Some(f64::MAX);
        assert!(merge(vec![copy]).is_err());
        let mut different = r.clone();
        different.key = "legacy_usage:1".into();
        assert_eq!(merge(vec![r, different]).unwrap().len(), 2);
    }
    #[test]
    fn reported_and_estimated_tokens_money_and_unknowns_remain_independent() {
        let mut r = receipt();
        let mut totals = Totals::default();
        r.attributed_usage=r.usage.clone();
        totals.add(&r);
        assert_eq!(totals.reported_usd, Some(0.));
        r.reported_usd = None;
        r.estimated_usd = Some(2.);
        let u = r.usage.as_mut().unwrap();
        u.source = UsageSource::Estimated;
        u.input_tokens = 9;
        u.credits = Some(100.);
        r.attributed_usage=r.usage.clone();
        totals.add(&r);
        assert_eq!(totals.reported_usd, None);
        assert_eq!(totals.known_reported_usd, 0.);
        assert_eq!(totals.known_estimated_usd, 2.);
        assert_eq!(totals.estimated_tokens.unwrap().input, 9);
        assert_eq!(totals.reported_tokens.unwrap().input, 0);
    }
    #[test]
    fn runtime_and_legacy_bridge_share_one_canonical_receipt_key() {
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/shared-runtime-v1.json"))
                .unwrap();
        let request:tokage_runtime::Request=serde_json::from_value(serde_json::json!({"schemaVersion":1,"commandId":"create","runId":"run","expectedRevision":0,"command":{"type":"CreateRun","payload":{"config":f["config"],"requirement":"accounting"}}})).unwrap();
        let (mut snapshot, _) = tokage_runtime::transition(None, &request).unwrap();
        let mut turn = tokage_runtime::turn::TurnRecord::prepared(
            "one".into(),
            "reviewer",
            None,
            snapshot.config.models["executor"].clone(),
        );
        turn.status = tokage_runtime::turn::TurnStatus::Completed;
        turn.ended = true;
        turn.cost_usd = Some(0.);
        turn.finished_at = Some(2000000);
        turn.usage = receipt().usage;
        snapshot.turns.insert("one".into(), turn);
        let runtime = tokage_runtime::accounting::project(&snapshot).unwrap();
        let mut duplicate = runtime.receipts[0].clone();
        duplicate.origin = "APP_RECEIPT".into();
        duplicate.evidence_digest = "usage_events:1".into();
        let rows = merge([runtime.receipts, vec![duplicate]].concat()).unwrap();
        let mut total = Totals::default();
        for r in &rows {
            total.add(r);
        }
        assert_eq!(total.receipts, 1);
        assert_eq!(total.reported_usd, Some(0.));
        assert_eq!(rows[0].phase, "REVIEW");
        assert_eq!(rows[0].duplicate_sources, vec!["usage_events:1"]);
    }
    #[test]
    fn production_read_has_partial_totals_signed_comparisons_and_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = tokage_storage::Storage::open(dir.path()).unwrap();
        let c = storage.read_conn().unwrap();
        c.execute_batch("INSERT INTO usage_events(provider_name,model_id,phase,source,input_tokens,cost_usd,equiv_cost_usd,credits) VALUES ('CODEX_CLI','m','EXECUTION','STRUCTURED',10,0,1,12),('CODEX_CLI','m','REPAIR','STRUCTURED',20,NULL,NULL,NULL),('CODEX_CLI','m','PLANNING','ESTIMATED',99,2,2,4); INSERT INTO savings_events(kind,baseline_usd,actual_usd,settled,detail_json) VALUES ('CONTEXT',1,2,1,'{\"basis\":\"first_turn_counterfactual_v1\",\"horizonTurns\":1}'),('ASSIGNMENT',10,1,1,'{}'),('CONTEXT',1,NULL,0,'{}');").unwrap();
        let before = c.total_changes();
        let view = crate::quality::usage_dashboard(&c, dir.path(), Some("7d")).unwrap();
        let a = view.accounting.as_ref().unwrap();
        assert_eq!(a.totals.receipts, 3);
        assert_eq!(a.totals.reported_usd, None);
        assert_eq!(a.totals.known_reported_usd, 0.);
        assert_eq!(a.totals.unknown_usd_receipts, 2);
        assert!(a.totals.reported_tokens.is_none());
        assert_eq!(a.totals.unknown_token_receipts, 2);
        assert_eq!(a.totals.estimated_tokens.as_ref().unwrap().input, 99);
        assert_eq!(a.comparable_count, 1);
        assert_eq!(
            a.comparisons
                .iter()
                .find(|c| c.kind == "CONTEXT" && c.delta_usd.is_some())
                .unwrap()
                .delta_usd,
            Some(-1.)
        );
        assert_eq!(view.savings.total_usd, None);
        let again = crate::quality::usage_dashboard(&c, dir.path(), Some("7d")).unwrap();
        assert_eq!(again.accounting.unwrap().totals.receipts, 3);
        assert_eq!(c.total_changes(), before);
        assert!(!dir.path().join("runtime-v1").exists());
        c.execute(
            "UPDATE usage_events SET created_at='unparseable' WHERE phase='EXECUTION'",
            [],
        )
        .unwrap();
        let view = crate::quality::usage_dashboard(&c, dir.path(), Some("today")).unwrap();
        let a = view.accounting.unwrap();
        assert_eq!(a.unknown_date_receipts, 1);
        assert_eq!(a.totals.reported_usd, None);
    }
}
