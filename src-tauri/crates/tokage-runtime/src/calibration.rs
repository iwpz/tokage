//! Versioned, idempotent local token-price calibration from accepted native receipts.
use crate::*;
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
const KEY: &str = "context.calibration.v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalFactor {
    pub price_key: String,
    pub factor: f64,
    pub samples: u32,
    pub minimum_samples: u32,
    pub source: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub price_key: String,
    pub provider: String,
    pub model: String,
    pub ratio: f64,
    pub evidence: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ledger {
    pub samples: BTreeMap<String, Sample>,
    pub ignored: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub price_key: String,
    pub provider: String,
    pub model: String,
    pub samples: u32,
    pub minimum_samples: u32,
    pub factor: Option<f64>,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub source: String,
    pub groups: Vec<Group>,
    pub eligible_samples: usize,
    pub capacity_limited: bool,
    pub ignored_reasons: BTreeMap<String, u32>,
    pub overrides: Vec<LocalFactor>,
}
pub fn price_key(
    pack: &tokage_matching::merge::EffectiveKnowledgePack,
    c: &tokage_matching::merge::EffectiveCandidate,
) -> Result<String> {
    let m = pack.models.get(&c.model_id).ok_or(Error::Transition)?;
    digest(&(
        "TOKEN_PRICE_EQUIVALENT_V1",
        &c.provider,
        &c.model_id,
        &m.price_per_mtok,
        &m.cache_rates,
        &m.long_context_pricing,
        &m.price_schedule,
        pack.providers
            .get(&c.provider)
            .and_then(|p| p.cache.as_ref()),
    ))
}
pub fn apply(
    pack: &mut tokage_matching::merge::EffectiveKnowledgePack,
    values: &[LocalFactor],
    minimum: u32,
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for v in values {
        if !seen.insert(v.price_key.clone())
            || v.source != "SETTLED_NATIVE_TOKEN_PRICE_V1"
            || !v.factor.is_finite()
            || !(0.2..=5.).contains(&v.factor)
            || v.minimum_samples < 3
            || v.samples < v.minimum_samples
            || v.samples < minimum
        {
            return Err(Error::Invalid(
                "Invalid or insufficient calibration evidence".into(),
            ));
        }
    }
    let factors = pack
        .candidates
        .iter()
        .map(|c| {
            let key = price_key(pack, c)?;
            let base = &pack.models[&c.model_id].price_per_mtok;
            Ok((
                values.iter().find(|v| v.price_key == key).map(|v| v.factor),
                base.input,
                base.output,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    for (c, (factor, input, output)) in pack.candidates.iter_mut().zip(factors) {
        let f = factor.unwrap_or(1.);
        c.price_per_mtok.input = input * f;
        c.price_per_mtok.output = output * f;
        c.origin_layer = c
            .origin_layer
            .split(" + SETTLED_NATIVE_TOKEN_PRICE_V1")
            .next()
            .unwrap_or(&c.origin_layer)
            .to_owned();
        if factor.is_some() {
            c.origin_layer.push_str(" + SETTLED_NATIVE_TOKEN_PRICE_V1");
        }
    }

    Ok(())
}
fn load(c: &Connection) -> Result<Ledger> {
    let json: Option<String> = c
        .query_row("SELECT value_json FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| Error::Repository(e.to_string()))?;
    let l: Ledger = match json {
        Some(s) => serde_json::from_str(&s).map_err(|e| Error::Repository(e.to_string()))?,
        None => Default::default(),
    };
    if l.samples.len() > 10000
        || l.ignored.len() > 10000
        || l.samples
            .values()
            .any(|s| !s.ratio.is_finite() || !(0.2..=5.).contains(&s.ratio))
    {
        return Err(Error::Invalid(
            "Calibration ledger invalid or exceeds bounds".into(),
        ));
    }
    Ok(l)
}
impl Ledger {
    pub fn status(&self, minimum: u32) -> Result<Status> {
        if !(3..=100).contains(&minimum) {
            return Err(Error::Invalid("Calibration minimum must be 3..100".into()));
        }
        let mut grouped: BTreeMap<String, Vec<&Sample>> = BTreeMap::new();
        for sample in self.samples.values() {
            grouped
                .entry(sample.price_key.clone())
                .or_default()
                .push(sample);
        }
        let mut groups = Vec::new();
        let mut overrides = Vec::new();
        for (key, mut samples) in grouped {
            samples.sort_by(|a, b| a.ratio.total_cmp(&b.ratio));
            let n = samples.len();
            let median = if n % 2 == 0 {
                (samples[n / 2 - 1].ratio + samples[n / 2].ratio) / 2.
            } else {
                samples[n / 2].ratio
            };
            let factor = (n >= minimum as usize).then_some(median);
            if let Some(factor) = factor {
                overrides.push(LocalFactor {
                    price_key: key.clone(),
                    factor,
                    samples: n as u32,
                    minimum_samples: minimum,
                    source: "SETTLED_NATIVE_TOKEN_PRICE_V1".into(),
                });
            }
            groups.push(Group {
                price_key: key,
                provider: samples[0].provider.clone(),
                model: samples[0].model.clone(),
                samples: n as u32,
                minimum_samples: minimum,
                factor,
                status: if factor.is_some() {
                    "READY_LOCAL_OVERRIDE"
                } else {
                    "INSUFFICIENT_SAMPLES"
                }
                .into(),
            });
        }
        let mut ignored_reasons = BTreeMap::new();
        for reason in self.ignored.values() {
            *ignored_reasons.entry(reason.clone()).or_default() += 1;
        }
        Ok(Status {
            source: "SETTLED_NATIVE_TOKEN_PRICE_V1".into(),
            groups,
            eligible_samples: self.samples.len(),
            capacity_limited:self.samples.len()>=10000||self.ignored.len()>=10000,
            ignored_reasons,
            overrides,
        })
    }
    pub fn ingest(&mut self, s: &Snapshot) -> Result<()> {
        for t in s.turns.values().filter(|t| t.role == "executor") {
            let key = format!("{}:{}:v1", s.run_id, t.id);
            let sampled = (|| -> std::result::Result<Sample, &'static str> {
                if !t.ended
                    || !t.cleanup_confirmed
                    || t.status != turn::TurnStatus::Completed
                    || t.error.is_some()
                {
                    return Err("NATIVE_NOT_SETTLED");
                }
                let task = t
                    .task_id
                    .as_ref()
                    .and_then(|id| s.execution.as_ref()?.tasks.get(id))
                    .ok_or("TASK_NOT_ACCEPTED")?;
                if task.state != tokage_orchestration::TaskState::Done
                    || task.attempts.last() != Some(&t.id)
                    || task.deviation
                    || task.manual.is_some()
                {
                    return Err("TASK_NOT_ACCEPTED");
                }
                let a = s
                    .work
                    .as_ref()
                    .and_then(|w| w.attempts.get(&t.id))
                    .ok_or("TASK_NOT_ACCEPTED")?;
                if a.commit.is_none()
                    || a.integrated_head.is_none()
                    || a.manual_receipt.is_some()
                    || a.gates.is_empty()
                    || a.gates.iter().any(|g| {
                        !g.cleanup_confirmed || g.result.as_ref().is_none_or(|r| !r.passed)
                    })
                {
                    return Err("TASK_NOT_ACCEPTED");
                }
                let launch = t.accounting_launch.as_ref().ok_or("LAUNCH_SCOPE_UNKNOWN")?;
                // Resumed non-Codex currency scope is not attested by the old adapters. Never infer billing from a token-equivalent.
                if launch.requested_thread.is_some() {
                    return Err("RESUMED_CURRENCY_SCOPE_UNVERIFIED");
                }
                let usage = crate::usage_attribution::project(s, t)
                    .usage
                    .ok_or("TOKEN_ATTRIBUTION_UNKNOWN")?;
                if usage.source != tokage_core::UsageSource::Structured {
                    return Err("ESTIMATED_USAGE");
                }
                let actual = t
                    .cost_usd
                    .filter(|v| v.is_finite() && *v >= 0.)
                    .ok_or("REPORTED_USD_UNKNOWN")?;
                if actual == 0. {
                    return Err("ZERO_USD_NOT_RATE_SAMPLE");
                }
                let price = t.price_basis.as_ref().ok_or("PRICE_UNKNOWN")?;
                let factor = price.calibration_factor.ok_or("LEGACY_PRICE_SCOPE")?;
                let price_key = price.calibration_key.clone().ok_or("LEGACY_PRICE_SCOPE")?;
                let observed = t.observed_model.as_ref().ok_or("OBSERVED_MODEL_UNKNOWN")?;
                if t.observed_provider
                    .as_ref()
                    .is_some_and(|p| p != &t.requested.provider)
                {
                    return Err("OBSERVED_PROVIDER_CHANGED");
                }
                let (estimate, _) = crate::accounting::estimate(
                    Some(price),
                    &t.requested.provider,
                    observed,
                    &usage,
                )
                .map_err(|_| "PRICE_UNKNOWN")?;
                let expected = estimate.filter(|v| *v > 0.).ok_or("PRICE_UNKNOWN")? / factor;
                let ratio = actual / expected;
                if !ratio.is_finite() || !(0.2..=5.).contains(&ratio) {
                    return Err("OUTLIER_RATIO");
                }
                let evidence = digest(&(
                    &t.requested,
                    &t.usage_snapshot,
                    &t.usage,
                    &t.accounting_launch,
                    price,
                    &a.commit,
                    &a.integrated_head,
                ))
                .map_err(|_| "INVALID_EVIDENCE")?;
                Ok(Sample {
                    price_key,
                    provider: t.requested.provider.clone(),
                    model: t.requested.requested_model.clone(),
                    ratio,
                    evidence,
                })
            })();
            match sampled {
                Ok(sample) => {
                    if let Some(old) = self.samples.get(&key) {
                        if old != &sample {
                            return Err(Error::Invalid(
                                "Settled calibration evidence changed".into(),
                            ));
                        }
                    } else if self.samples.len() < 10000 {
                        self.samples.insert(key.clone(), sample);
                    } else {
                        if self.ignored.len()<10000 { self.ignored
                            .insert(key.clone(), "SAMPLE_CAPACITY_REACHED".into()); }
                        continue;
                    }
                    self.ignored.remove(&key);
                }
                Err(reason) => {
                    if !self.samples.contains_key(&key) && self.ignored.len() < 10000 {
                        self.ignored.insert(key, reason.into());
                    }
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn persist(c: &Connection, s: &Snapshot) -> rusqlite::Result<()> {
    let err = |e: Error| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
    let mut ledger = load(c).map_err(err)?;
    let old = ledger.clone();
    ledger.ingest(s).map_err(err)?;
    if ledger != old {
        let json = serde_json::to_string(&ledger)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        c.execute("INSERT INTO settings(key,value_json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",rusqlite::params![KEY,json])?;
    }
    Ok(())
}
pub fn read(directory: &Path, minimum: u32) -> Result<Status> {
    if !directory.join("tokage.db").exists() {
        return Ledger::default().status(minimum);
    }
    if std::fs::symlink_metadata(directory.join("tokage.db"))
        .map_err(|e| Error::Repository(e.to_string()))?
        .file_type()
        .is_symlink()
    {
        return Err(Error::Invalid("Symlinked runtime database".into()));
    }
    let conn = Connection::open_with_flags(
        directory.join("tokage.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| Error::Repository(e.to_string()))?;
    load(&conn)?.status(minimum)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settled() -> Snapshot {
        let (mut s, _) = task_context::tests::fixture();
        let id = "b";
        let task = s.execution.as_mut().unwrap().tasks.get_mut("T1").unwrap();
        task.state = tokage_orchestration::TaskState::Done;
        task.attempts = vec!["b".into()];
        let mut selected = s.turns[id].requested.clone();
        selected.provider = "CLAUDE_CODE".into();
        selected.requested_model = "claude-sonnet-4-6".into();
        let mut price = accounting::price_basis(&s.config, &s.turns[id].requested)
            .unwrap()
            .unwrap();
        price.provider = selected.provider.clone();
        price.model = selected.requested_model.clone();
        price.input = 2.;
        price.output = 10.;
        price.long_context = None;
        price.cache_read = Some(0.1);
        price.calibration_factor = Some(1.);
        price.calibration_key = Some("price-regime".into());
        let t = s.turns.get_mut(id).unwrap();
        t.requested = selected;
        t.observed_model = Some(t.requested.requested_model.clone());
        t.accounting_launch = Some(usage_attribution::Launch {
            resume_source: None,
            requested_thread: None,
        });
        t.price_basis = Some(price);
        t.usage = Some(tokage_core::TurnUsage {
            input_tokens: 1000000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 0,
            api_calls: Some(1),
            context_tokens: Some(1000000),
            cost_usd: Some(4.),
            credits: None,
            premium_requests: None,
            source: tokage_core::UsageSource::Structured,
        });
        t.cost_usd = Some(4.);
        let scope = tokage_native::git::scope::ScopeSnapshot {
            root: "/tree".into(),
            workdir: "/tree".into(),
            prefix: String::new(),
            head: "head".into(),
            clean: true,
            files: Default::default(),
        };
        s.work.as_mut().unwrap().attempts.insert(
            id.into(),
            artifacts::AttemptEvidence {
                commit: Some("commit".into()),
                integrated_head: Some("integrated".into()),
                gates: vec![artifacts::GateEvidence {
                    id: "gate".into(),
                    command: "check".into(),
                    before: scope.clone(),
                    after: Some(scope),
                    result: Some(tokage_orchestration::GateCommandResult {
                        command: "check".into(),
                        duration_ms: 0,
                        passed: true,
                        exit_code: Some(0),
                        output_tail: "ok".into(),
                        error_message: None,
                    }),
                    cleanup_confirmed: true,
                }],
                ..Default::default()
            },
        );
        s
    }
    #[test]
    fn durable_ledger_reopens_and_does_not_learn_its_own_applied_factor(){
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("tokage.db");
        let mut ledger=Ledger::default();let mut s=settled();
        // An existing factor doubles the frozen quote. Divide it out before fitting again.
        {let p=s.turns.get_mut("b").unwrap().price_basis.as_mut().unwrap();p.input*=2.;p.output*=2.;p.calibration_factor=Some(2.);}
        for i in 0..5 {s.run_id=format!("durable-{i}");ledger.ingest(&s).unwrap();}
        let c=Connection::open(&path).unwrap();c.execute("CREATE TABLE settings(key TEXT PRIMARY KEY,value_json TEXT)",[]).unwrap();
        c.execute("INSERT INTO settings VALUES(?1,?2)",rusqlite::params![KEY,serde_json::to_string(&ledger).unwrap()]).unwrap();drop(c);
        let status=read(dir.path(),5).unwrap();assert_eq!(status.overrides[0].factor,2.);assert_eq!(status.eligible_samples,5);
        assert_eq!(read(dir.path(),5).unwrap().eligible_samples,5);
    }
    #[test]
    fn settled_samples_are_idempotent_scoped_and_require_a_minimum() {
        let mut ledger = Ledger::default();
        let mut s = settled();
        for i in 0..5 {
            s.run_id = format!("run-{i}");
            ledger.ingest(&s).unwrap();
            ledger.ingest(&s).unwrap();
        }
        assert_eq!(ledger.samples.len(), 5);
        assert_eq!(ledger.status(5).unwrap().overrides[0].factor, 2.);
        assert!(ledger.status(6).unwrap().overrides.is_empty());
        s.turns.get_mut("b").unwrap().cost_usd = Some(3.);
        assert!(ledger.ingest(&s).is_err());
    }
    #[test]
    fn missing_cost_estimated_legacy_resumed_and_outlier_observations_never_train() {
        for variant in 0..7 {
            let mut s = settled();
            match variant {
                0 => s.turns.get_mut("b").unwrap().cost_usd = None,
                1 => {
                    s.turns
                        .get_mut("b")
                        .unwrap()
                        .price_basis
                        .as_mut()
                        .unwrap()
                        .calibration_key = None
                }
                2 => {
                    s.turns.get_mut("b").unwrap().usage.as_mut().unwrap().source =
                        tokage_core::UsageSource::Estimated
                }
                3 => {
                    s.turns
                        .get_mut("b")
                        .unwrap()
                        .accounting_launch
                        .as_mut()
                        .unwrap()
                        .requested_thread = Some("old".into())
                }
                4 => s.turns.get_mut("b").unwrap().cost_usd = Some(999.),
                5 => {
                    s.work
                        .as_mut()
                        .unwrap()
                        .attempts
                        .get_mut("b")
                        .unwrap()
                        .commit = None
                }
                _ => s.turns.get_mut("b").unwrap().cleanup_confirmed = false,
            }
            let mut ledger = Ledger::default();
            ledger.ingest(&s).unwrap();
            assert!(ledger.samples.is_empty(), "case {variant}");
        }
    }
    #[test]
    fn persistence_reopens_without_double_learning_and_price_changes_do_not_share_overrides() {
        let c = Connection::open_in_memory().unwrap();
        c.execute(
            "CREATE TABLE settings(key TEXT PRIMARY KEY,value_json TEXT)",
            [],
        )
        .unwrap();
        let s = settled();
        persist(&c, &s).unwrap();
        let changes = c.total_changes();
        persist(&c, &s).unwrap();
        assert_eq!(c.total_changes(), changes);
        assert_eq!(load(&c).unwrap().samples.len(), 1);
        let mut pack = planning::effective_pack(&s.config).unwrap();
        let key = price_key(&pack, &pack.candidates[0]).unwrap();
        let value = LocalFactor {
            price_key: key.clone(),
            factor: 2.,
            samples: 5,
            minimum_samples: 5,
            source: "SETTLED_NATIVE_TOKEN_PRICE_V1".into(),
        };
        let base = pack.candidates[0].price_per_mtok.input;
        apply(&mut pack, &[value.clone()], 5).unwrap();
        assert_eq!(pack.candidates[0].price_per_mtok.input, base * 2.);
        apply(&mut pack, &[value.clone()], 5).unwrap();
        assert_eq!(pack.candidates[0].price_per_mtok.input, base * 2.);
        let model = pack.candidates[0].model_id.clone();
        pack.models.get_mut(&model).unwrap().price_per_mtok.input += 1.;
        apply(&mut pack, &[value], 5).unwrap();
        assert_ne!(price_key(&pack, &pack.candidates[0]).unwrap(), key);
        assert_eq!(pack.candidates[0].price_per_mtok.input, base + 1.);
    }
}
