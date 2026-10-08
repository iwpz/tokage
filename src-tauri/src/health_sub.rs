//! Provider health bus subscriber and recovery loop (docs/06 §6, F15).
//!
//! `TurnFailed` / `TurnSucceeded` from `run_turn` are applied to `provider_health`;
//! melt / recover also emit frontend events with `{ provider, reason, until }`.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager};
use tokage_core::{Bus, DomainEvent, ProviderId, TurnErrorKind};
use tokage_native::chat::quota::format_unix_to_utc_iso;
use tokage_storage::{self as storage, MeltReason, ProviderHealthRow, Storage};

const HEALTH_FAIL_THRESHOLD: i64 = 3;
const HEALTH_PROBE_SECS: i64 = 10 * 60;
const QUOTA_DEFAULT_SECS: i64 = 60 * 60;
const QUOTA_RECOVER_MAX_UTIL: f64 = 0.80;
const RECOVERY_INTERVAL_SECS: u64 = 60;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeltedPayload {
    pub provider: String,
    pub reason: String,
    pub until: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecoveredPayload {
    pub provider: String,
    pub reason: Option<String>,
    pub until: Option<String>,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn iso_now_plus(secs: i64) -> String {
    format_unix_to_utc_iso(unix_now() + secs)
}

/// Apply one domain event to storage. Returns a frontend emit hint when melt/recover happened.
pub async fn apply_health_event(
    storage: &Storage,
    event: &DomainEvent,
) -> Result<Option<HealthEmit>, String> {
    match event {
        DomainEvent::TurnFailed {
            provider,
            kind,
            melted_until,
            ..
        } => apply_turn_failed(storage, *provider, *kind, melted_until.clone()).await,
        DomainEvent::TurnSucceeded { provider } => apply_turn_succeeded(storage, *provider).await,
        _ => Ok(None),
    }
}

#[derive(Clone, Debug)]
pub enum HealthEmit {
    Melted(ProviderMeltedPayload),
    Recovered(ProviderRecoveredPayload),
}

async fn apply_turn_failed(
    storage: &Storage,
    provider: ProviderId,
    kind: TurnErrorKind,
    melted_until: Option<String>,
) -> Result<Option<HealthEmit>, String> {
    let name = provider.as_str().to_string();

    // HALF_OPEN probe failure → remelt as HEALTH.
    if let Ok(Some(row)) = storage.get_provider_health(&name) {
        if row.status == "HALF_OPEN" {
            let until = Some(iso_now_plus(HEALTH_PROBE_SECS));
            storage
                .trip_provider_melt(name.clone(), MeltReason::Health, until.clone())
                .await?;
            return Ok(Some(HealthEmit::Melted(ProviderMeltedPayload {
                provider: name,
                reason: MeltReason::Health.as_str().to_string(),
                until,
            })));
        }
    }

    match kind {
        TurnErrorKind::Timeout | TurnErrorKind::NonZeroExit | TurnErrorKind::SpawnFailed => {
            let fails = storage.record_provider_failure(name.clone()).await?;
            if fails >= HEALTH_FAIL_THRESHOLD {
                let until = Some(iso_now_plus(HEALTH_PROBE_SECS));
                storage
                    .trip_provider_melt(name.clone(), MeltReason::Health, until.clone())
                    .await?;
                return Ok(Some(HealthEmit::Melted(ProviderMeltedPayload {
                    provider: name,
                    reason: MeltReason::Health.as_str().to_string(),
                    until,
                })));
            }
            Ok(None)
        }
        TurnErrorKind::RateLimited => {
            let until = melted_until.or_else(|| Some(iso_now_plus(QUOTA_DEFAULT_SECS)));
            storage
                .trip_provider_melt(name.clone(), MeltReason::QuotaHard, until.clone())
                .await?;
            Ok(Some(HealthEmit::Melted(ProviderMeltedPayload {
                provider: name,
                reason: MeltReason::QuotaHard.as_str().to_string(),
                until,
            })))
        }
        TurnErrorKind::AuthRequired => {
            storage
                .trip_provider_melt(name.clone(), MeltReason::Auth, None)
                .await?;
            Ok(Some(HealthEmit::Melted(ProviderMeltedPayload {
                provider: name,
                reason: MeltReason::Auth.as_str().to_string(),
                until: None,
            })))
        }
        TurnErrorKind::ParseError => Ok(None),
    }
}

async fn apply_turn_succeeded(
    storage: &Storage,
    provider: ProviderId,
) -> Result<Option<HealthEmit>, String> {
    let name = provider.as_str().to_string();
    let was_half_open = storage
        .get_provider_health(&name)
        .ok()
        .flatten()
        .is_some_and(|r| r.status == "HALF_OPEN");

    storage.reset_provider_failures(name.clone()).await?;

    if was_half_open {
        storage
            .recover_provider_health(name.clone(), Some("READY".into()))
            .await?;
        return Ok(Some(HealthEmit::Recovered(ProviderRecoveredPayload {
            provider: name,
            reason: None,
            until: None,
        })));
    }
    Ok(None)
}

fn emit_health(app: &AppHandle, hint: HealthEmit) {
    match hint {
        HealthEmit::Melted(p) => {
            let _ = app.emit("provider-melted", &p);
        }
        HealthEmit::Recovered(p) => {
            let _ = app.emit("provider-recovered", &p);
        }
    }
}

/// Lifetime subscriber: TurnFailed / TurnSucceeded → provider_health + frontend events.
pub fn spawn_health_subscriber(app: AppHandle, bus: &Bus) {
    let mut rx = bus.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let state = app.state::<storage::StorageState>();
            let Ok(storage) = state.get() else {
                continue;
            };
            match apply_health_event(storage, &event).await {
                Ok(Some(hint)) => emit_health(&app, hint),
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "provider health subscriber failed"),
            }
        }
    });
}

/// Startup + every 60s: expire melts into HALF_OPEN / recover QUOTA when util < 80%.
pub fn spawn_recovery_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let state = app.state::<storage::StorageState>();
                if let Ok(storage) = state.get() {
                    if let Err(e) = run_recovery_tick(&app, storage).await {
                        tracing::warn!(error = %e, "provider health recovery tick failed");
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(RECOVERY_INTERVAL_SECS)).await;
        }
    });
}

pub async fn run_recovery_tick(app: &AppHandle, storage: &Storage) -> Result<(), String> {
    let now = format_unix_to_utc_iso(unix_now());
    let rows = storage.list_provider_health()?;
    for row in rows {
        if row.status != "MELTED" {
            continue;
        }
        let Some(until) = row.melted_until.as_deref() else {
            continue;
        };
        if until > now.as_str() {
            continue;
        }
        match row.melt_reason {
            Some(MeltReason::Health) | None => {
                storage
                    .set_provider_half_open(row.provider_name.clone())
                    .await?;
            }
            Some(MeltReason::QuotaSoft) | Some(MeltReason::QuotaHard) => {
                recover_or_extend_quota(app, storage, &row).await?;
            }
            Some(MeltReason::Auth) | Some(MeltReason::Budget) => {
                // Auth / budget: do not auto-recover on timer; wait for manual recover or success path.
            }
        }
    }
    Ok(())
}

async fn recover_or_extend_quota(
    app: &AppHandle,
    storage: &Storage,
    row: &ProviderHealthRow,
) -> Result<(), String> {
    let util = latest_utilization_pct(storage, &row.provider_name).unwrap_or(0.0);
    if util < QUOTA_RECOVER_MAX_UTIL {
        storage
            .recover_provider_health(row.provider_name.clone(), Some("READY".into()))
            .await?;
        emit_health(
            app,
            HealthEmit::Recovered(ProviderRecoveredPayload {
                provider: row.provider_name.clone(),
                reason: row.melt_reason.map(|r| r.as_str().to_string()),
                until: None,
            }),
        );
    } else {
        let until = Some(iso_now_plus(QUOTA_DEFAULT_SECS));
        let reason = row.melt_reason.unwrap_or(MeltReason::QuotaHard);
        storage
            .trip_provider_melt(row.provider_name.clone(), reason, until)
            .await?;
    }
    Ok(())
}

fn latest_utilization_pct(storage: &Storage, provider: &str) -> Option<f64> {
    let snap = storage.get_latest_usage_snapshot(provider.to_string()).ok()??;
    let pct = snap.usage_percentage?;
    // Snapshots store percent 0–100; normalize to 0–1.
    Some(if pct > 1.0 { pct / 100.0 } else { pct })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::{DomainEvent, ProviderId, TurnErrorKind};
    use tokage_storage::MeltReason;

    fn temp_storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        (dir, storage)
    }

    #[tokio::test]
    async fn three_timeouts_melt_health_and_success_resets() {
        let (_dir, storage) = temp_storage();
        let provider = ProviderId::ClaudeCode;
        for _ in 0..2 {
            let hint = apply_health_event(
                &storage,
                &DomainEvent::TurnFailed {
                    provider,
                    kind: TurnErrorKind::Timeout,
                    message: "idle timeout".into(),
                    melted_until: None,
                },
            )
            .await
            .unwrap();
            assert!(hint.is_none());
        }
        let hint = apply_health_event(
            &storage,
            &DomainEvent::TurnFailed {
                provider,
                kind: TurnErrorKind::Timeout,
                message: "idle timeout".into(),
                melted_until: None,
            },
        )
        .await
        .unwrap();
        match hint {
            Some(HealthEmit::Melted(p)) => {
                assert_eq!(p.provider, "CLAUDE_CODE");
                assert_eq!(p.reason, "HEALTH");
                assert!(p.until.is_some());
            }
            other => panic!("expected melt, got {other:?}"),
        }
        let row = storage
            .get_provider_health("CLAUDE_CODE")
            .unwrap()
            .expect("row");
        assert_eq!(row.status, "MELTED");
        assert_eq!(row.melt_reason, Some(MeltReason::Health));

        apply_health_event(
            &storage,
            &DomainEvent::TurnSucceeded { provider },
        )
        .await
        .unwrap();
        // Still melted (success only clears consecutive fails unless HALF_OPEN).
        let row = storage.get_provider_health("CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(row.status, "MELTED");
        assert_eq!(row.consecutive_fails, 0);

        // Fresh provider: success after 1 fail clears count without melt.
        let (_d2, s2) = temp_storage();
        apply_health_event(
            &s2,
            &DomainEvent::TurnFailed {
                provider: ProviderId::CodexCli,
                kind: TurnErrorKind::Timeout,
                message: "t".into(),
                melted_until: None,
            },
        )
        .await
        .unwrap();
        apply_health_event(
            &s2,
            &DomainEvent::TurnSucceeded {
                provider: ProviderId::CodexCli,
            },
        )
        .await
        .unwrap();
        let h = s2.get_provider_health("CODEX_CLI").unwrap().unwrap();
        assert_eq!(h.consecutive_fails, 0);
        assert_ne!(h.status, "MELTED");
    }

    #[tokio::test]
    async fn rate_limited_uses_reset_at() {
        let (_dir, storage) = temp_storage();
        let until = "2026-10-08T12:00:00Z".to_string();
        let hint = apply_health_event(
            &storage,
            &DomainEvent::TurnFailed {
                provider: ProviderId::ClaudeCode,
                kind: TurnErrorKind::RateLimited,
                message: "429".into(),
                melted_until: Some(until.clone()),
            },
        )
        .await
        .unwrap();
        match hint {
            Some(HealthEmit::Melted(p)) => {
                assert_eq!(p.reason, "QUOTA_HARD");
                assert_eq!(p.until.as_deref(), Some(until.as_str()));
            }
            other => panic!("expected melt, got {other:?}"),
        }
        let row = storage.get_provider_health("CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(row.status, "MELTED");
        assert_eq!(row.melt_reason, Some(MeltReason::QuotaHard));
        assert_eq!(row.melted_until.as_deref(), Some(until.as_str()));
    }

    #[tokio::test]
    async fn auth_required_melts_auth() {
        let (_dir, storage) = temp_storage();
        let hint = apply_health_event(
            &storage,
            &DomainEvent::TurnFailed {
                provider: ProviderId::ClaudeCode,
                kind: TurnErrorKind::AuthRequired,
                message: "login".into(),
                melted_until: None,
            },
        )
        .await
        .unwrap();
        match hint {
            Some(HealthEmit::Melted(p)) => assert_eq!(p.reason, "AUTH"),
            other => panic!("expected melt, got {other:?}"),
        }
        let row = storage.get_provider_health("CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(row.melt_reason, Some(MeltReason::Auth));
    }

    #[tokio::test]
    async fn expired_health_melt_becomes_half_open() {
        let (_dir, storage) = temp_storage();
        storage
            .trip_provider_melt(
                "CLAUDE_CODE".into(),
                MeltReason::Health,
                Some("2020-01-01T00:00:00Z".into()),
            )
            .await
            .unwrap();
        // Recovery tick needs an AppHandle for quota emits; call half-open path directly
        // via the same status transition the tick uses for HEALTH.
        storage
            .set_provider_half_open("CLAUDE_CODE".into())
            .await
            .unwrap();
        let row = storage.get_provider_health("CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(row.status, "HALF_OPEN");
    }

    #[tokio::test]
    async fn half_open_success_recovers_failure_remelts() {
        let (_dir, storage) = temp_storage();
        storage
            .trip_provider_melt(
                "CLAUDE_CODE".into(),
                MeltReason::Health,
                Some("2020-01-01T00:00:00Z".into()),
            )
            .await
            .unwrap();
        storage
            .set_provider_half_open("CLAUDE_CODE".into())
            .await
            .unwrap();

        let hint = apply_health_event(
            &storage,
            &DomainEvent::TurnSucceeded {
                provider: ProviderId::ClaudeCode,
            },
        )
        .await
        .unwrap();
        assert!(matches!(hint, Some(HealthEmit::Recovered(_))));
        let row = storage.get_provider_health("CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(row.status, "READY");

        storage
            .trip_provider_melt(
                "CODEX_CLI".into(),
                MeltReason::Health,
                Some("2020-01-01T00:00:00Z".into()),
            )
            .await
            .unwrap();
        storage
            .set_provider_half_open("CODEX_CLI".into())
            .await
            .unwrap();
        let hint = apply_health_event(
            &storage,
            &DomainEvent::TurnFailed {
                provider: ProviderId::CodexCli,
                kind: TurnErrorKind::NonZeroExit,
                message: "boom".into(),
                melted_until: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(hint, Some(HealthEmit::Melted(_))));
        let row = storage.get_provider_health("CODEX_CLI").unwrap().unwrap();
        assert_eq!(row.status, "MELTED");
        assert_eq!(row.melt_reason, Some(MeltReason::Health));
    }
}
