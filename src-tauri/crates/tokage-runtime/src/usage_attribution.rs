//! Per-launch token attribution. Raw native counters are never overwritten or summed as turn costs.
use crate::*;
use tokage_core::{TurnUsage, UsageSource};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Launch {
    pub resume_source: Option<String>,
    pub requested_thread: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub usage: Option<TurnUsage>,
    pub basis: String,
    pub source_turn: Option<String>,
}
fn unknown(reason: &str) -> Attribution {
    Attribution {
        usage: None,
        basis: reason.into(),
        source_turn: None,
    }
}
fn total(t: &turn::TurnRecord) -> Option<&tokage_core::event::NativeTokenTotal> {
    t.usage
        .as_ref()
        .filter(|u| u.source == UsageSource::Structured)?;
    let snapshot = t.usage_snapshot.as_ref()?;
    if !matches!(
        snapshot.protocol.as_str(),
        "CODEX_EXEC_THREAD_TOTAL_V1" | "CODEX_APP_SERVER_THREAD_TOTAL_V1"
    ) {
        return None;
    }
    snapshot.total.as_ref()
}
pub fn project(s: &Snapshot, t: &turn::TurnRecord) -> Attribution {
    if t.requested.provider != "CODEX_CLI"
        || t.usage
            .as_ref()
            .is_some_and(|u| u.source == UsageSource::Estimated)
    {
        return Attribution {
            usage: t.usage.clone(),
            basis: "PROVIDER_TURN_RECEIPT_OR_CLI_ESTIMATE".into(),
            source_turn: None,
        };
    }
    if !t.ended
        || !t.cleanup_confirmed
        || t.status != turn::TurnStatus::Completed
        || t.error.is_some()
    {
        return unknown("NATIVE_TURN_NOT_SETTLED");
    }
    let Some(now) = total(t) else {
        return unknown("CODEX_TOTAL_SCOPE_UNKNOWN_OR_LAST_API_ONLY");
    };
    let Some(launch) = t.accounting_launch.as_ref() else {
        return unknown("LEGACY_LAUNCH_SCOPE_UNKNOWN");
    };
    let Some(thread) = t.thread_id.as_ref().filter(|id| !id.trim().is_empty()) else {
        return unknown("NATIVE_THREAD_UNKNOWN");
    };
    let mut input = now.input_including_cache;
    let mut read = now.cache_read;
    let mut write = now.cache_write;
    let mut output = now.output;
    let mut source_id = None;
    let basis = if let Some(expected) = &launch.requested_thread {
        if thread != expected {
            return unknown("RESUMED_THREAD_CHANGED");
        }
        let Some(source) = launch.resume_source.as_ref().and_then(|id| s.turns.get(id)) else {
            return unknown("CUMULATIVE_BASELINE_MISSING");
        };
        if source.id == t.id
            || source.requested != t.requested
            || source.thread_id.as_ref() != Some(thread)
            || !source.ended
            || !source.cleanup_confirmed
            || source.status != turn::TurnStatus::Completed
            || source.error.is_some()
            || source
                .finished_at
                .zip(t.created_at)
                .is_none_or(|(end, start)| end > start)
            || source.observed_model != t.observed_model
            || source.observed_provider != t.observed_provider
        {
            return unknown("CUMULATIVE_BASELINE_INCOMPATIBLE");
        }
        let Some(old) = total(source) else {
            return unknown("CUMULATIVE_BASELINE_SCOPE_UNKNOWN");
        };
        if old.cache_read > old.input_including_cache || old.cache_write != 0 {
            return unknown("CUMULATIVE_BASELINE_INVALID_OR_CACHE_WRITE_UNKNOWN");
        }
        let Some(delta) = input
            .checked_sub(old.input_including_cache)
            .zip(read.checked_sub(old.cache_read))
            .zip(write.checked_sub(old.cache_write))
            .zip(output.checked_sub(old.output))
        else {
            return unknown("CUMULATIVE_COUNTER_RESET");
        };
        (((input, read), write), output) = delta;
        source_id = Some(source.id.clone());
        "CODEX_THREAD_TOTAL_DELTA_NONCACHED_INPUT"
    } else {
        if launch.resume_source.is_some() {
            return unknown("LAUNCH_SOURCE_WITHOUT_THREAD");
        }
        if s.turns.values().any(|other| {
            other.id != t.id
                && other.requested.provider == t.requested.provider
                && other.thread_id.as_ref() == Some(thread)
                && other
                    .accounting_launch
                    .as_ref()
                    .is_some_and(|l| l.requested_thread.is_none())
        }) {
            return unknown("FRESH_THREAD_COLLISION");
        }
        "CODEX_FRESH_THREAD_TOTAL_NONCACHED_INPUT"
    };
    // Codex input includes cache reads. Cache-write overlap is not verified, so do not invent its split.
    if write != 0 {
        return unknown("CODEX_CACHE_WRITE_OVERLAP_UNKNOWN");
    }
    let Some(non_cached) = input.checked_sub(read) else {
        return unknown("CODEX_CACHE_READ_EXCEEDS_INPUT");
    };
    Attribution {
        usage: Some(TurnUsage {
            input_tokens: non_cached,
            cache_read_tokens: read,
            cache_write_tokens: write,
            output_tokens: output,
            api_calls: None,
            context_tokens: None,
            cost_usd: None,
            premium_requests: None,
            credits: None,
            source: UsageSource::Structured,
        }),
        basis: basis.into(),
        source_turn: source_id,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> Snapshot {
        let (mut s, _) = crate::task_context::tests::fixture();
        for (id, created, end, input, cache, output) in
            [("a", 1, 10, 100, 20, 7), ("b", 15, 20, 180, 50, 12)]
        {
            let t = s.turns.get_mut(id).unwrap();
            t.created_at = Some(created);
            t.finished_at = Some(end);
            t.accounting_launch = Some(Launch {
                resume_source: if id == "b" { Some("a".into()) } else { None },
                requested_thread: if id == "b" {
                    Some("native-window".into())
                } else {
                    None
                },
            });
            t.usage_snapshot = Some(tokage_core::event::NativeUsageSnapshot {
                protocol: "CODEX_EXEC_THREAD_TOTAL_V1".into(),
                total: Some(tokage_core::event::NativeTokenTotal {
                    input_including_cache: input,
                    cache_read: cache,
                    cache_write: 0,
                    output,
                }),
            });
        }
        s
    }
    #[test]
    fn cumulative_delta_and_cache_subset_are_attributed_once_without_mutating_raw() {
        let s = fixture();
        let before = serde_json::to_value(&s).unwrap();
        let first = project(&s, &s.turns["a"]);
        let second = project(&s, &s.turns["b"]);
        let a = first.usage.unwrap();
        assert_eq!(
            (a.input_tokens, a.cache_read_tokens, a.output_tokens),
            (80, 20, 7)
        );
        let b = second.usage.unwrap();
        assert_eq!(
            (b.input_tokens, b.cache_read_tokens, b.output_tokens),
            (50, 30, 5)
        );
        assert_eq!(second.source_turn.as_deref(), Some("a"));
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
        assert_eq!(
            crate::accounting::project(&s).unwrap(),
            crate::accounting::project(&s).unwrap()
        );
    }
    #[test]
    fn missing_scope_reset_foreign_thread_and_unsettled_sources_stay_unknown() {
        for variant in 0..8 {
            let mut s = fixture();
            match variant {
                0 => s.turns.get_mut("b").unwrap().usage_snapshot = None,
                1 => {
                    s.turns
                        .get_mut("b")
                        .unwrap()
                        .usage_snapshot
                        .as_mut()
                        .unwrap()
                        .total
                        .as_mut()
                        .unwrap()
                        .input_including_cache = 1
                }
                2 => s.turns.get_mut("a").unwrap().thread_id = Some("other".into()),
                3 => s.turns.get_mut("a").unwrap().usage_snapshot = None,
                4 => s.turns.get_mut("a").unwrap().cleanup_confirmed = false,
                5 => s.turns.get_mut("b").unwrap().accounting_launch = None,
                6 => {
                    s.turns
                        .get_mut("b")
                        .unwrap()
                        .usage_snapshot
                        .as_mut()
                        .unwrap()
                        .total
                        .as_mut()
                        .unwrap()
                        .cache_write = 1
                }
                _ => s.turns.get_mut("b").unwrap().status = turn::TurnStatus::Failed,
            }
            assert!(
                project(&s, &s.turns["b"]).usage.is_none(),
                "mutation {variant}"
            );
        }
    }
    #[test]
    fn appserver_total_is_distinct_from_last_api_and_verified_zero_is_not_unknown() {
        let mut s = fixture();
        let old = s.turns["a"].usage_snapshot.as_ref().unwrap().total.clone();
        let b = s.turns.get_mut("b").unwrap();
        b.usage_snapshot = Some(tokage_core::event::NativeUsageSnapshot {
            protocol: "CODEX_APP_SERVER_THREAD_TOTAL_V1".into(),
            total: old,
        });
        let u = project(&s, &s.turns["b"]).usage.unwrap();
        assert_eq!(u.input_tokens + u.cache_read_tokens + u.output_tokens, 0);
        s.turns
            .get_mut("b")
            .unwrap()
            .usage_snapshot
            .as_mut()
            .unwrap()
            .total = None;
        assert!(project(&s, &s.turns["b"]).usage.is_none());
    }
}
