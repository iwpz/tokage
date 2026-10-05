//! Health status and melting circuit breaker for LLM/CLI providers (docs/06 §6, docs/08 schema.md §4, P8.m2c).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Reasons why a provider's circuit is tripped / melted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MeltReason {
    QuotaSoft,
    QuotaHard,
    Budget,
    Health,
    Auth,
}

impl MeltReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::QuotaSoft => "QUOTA_SOFT",
            Self::QuotaHard => "QUOTA_HARD",
            Self::Budget => "BUDGET",
            Self::Health => "HEALTH",
            Self::Auth => "AUTH",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "QUOTA_SOFT" => Some(Self::QuotaSoft),
            "QUOTA_HARD" => Some(Self::QuotaHard),
            "BUDGET" => Some(Self::Budget),
            "HEALTH" => Some(Self::Health),
            "AUTH" => Some(Self::Auth),
            _ => None,
        }
    }
}

/// Row in `provider_health` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealthRow {
    pub provider_name: String,
    pub position: i64,
    pub status: String,
    pub cli_version: Option<String>,
    pub info_json: String,
    pub melt_reason: Option<MeltReason>,
    pub melted_until: Option<String>,
    pub consecutive_fails: i64,
    pub updated_at: Option<String>,
}

/// Retrieves the health status row for a provider.
pub fn get_provider_health(conn: &Connection, provider_name: &str) -> rusqlite::Result<Option<ProviderHealthRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, position, status, cli_version, info_json, melt_reason, melted_until, consecutive_fails, updated_at
         FROM provider_health WHERE provider_name = ?1",
    )?;

    stmt.query_row(params![provider_name], |row| {
        let reason_str: Option<String> = row.get(5)?;
        let melt_reason = reason_str.as_deref().and_then(MeltReason::from_str);
        Ok(ProviderHealthRow {
            provider_name: row.get(0)?,
            position: row.get(1)?,
            status: row.get(2)?,
            cli_version: row.get(3)?,
            info_json: row.get(4)?,
            melt_reason,
            melted_until: row.get(6)?,
            consecutive_fails: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })
    .optional()
}

/// Lists all provider health records.
pub fn list_provider_health(conn: &Connection) -> rusqlite::Result<Vec<ProviderHealthRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, position, status, cli_version, info_json, melt_reason, melted_until, consecutive_fails, updated_at
         FROM provider_health ORDER BY position ASC, provider_name ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        let reason_str: Option<String> = row.get(5)?;
        let melt_reason = reason_str.as_deref().and_then(MeltReason::from_str);
        Ok(ProviderHealthRow {
            provider_name: row.get(0)?,
            position: row.get(1)?,
            status: row.get(2)?,
            cli_version: row.get(3)?,
            info_json: row.get(4)?,
            melt_reason,
            melted_until: row.get(6)?,
            consecutive_fails: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

/// Trips the circuit breaker for a provider, recording melt reason and optional reset time.
pub fn trip_provider_melt(
    conn: &Connection,
    provider_name: &str,
    reason: MeltReason,
    melted_until: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO provider_health (
            provider_name, status, info_json, melt_reason, melted_until, consecutive_fails, updated_at
        ) VALUES (
            ?1, 'MELTED', '{}', ?2, ?3, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now')
        ) ON CONFLICT(provider_name) DO UPDATE SET
            status = 'MELTED',
            melt_reason = ?2,
            melted_until = ?3,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![provider_name, reason.as_str(), melted_until],
    )?;
    Ok(())
}

/// Increments consecutive failure counter by 1. Returns updated failure count.
pub fn record_provider_failure(conn: &Connection, provider_name: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO provider_health (
            provider_name, status, info_json, consecutive_fails, updated_at
        ) VALUES (
            ?1, 'UNKNOWN', '{}', 1, strftime('%Y-%m-%dT%H:%M:%fZ','now')
        ) ON CONFLICT(provider_name) DO UPDATE SET
            consecutive_fails = consecutive_fails + 1,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![provider_name],
    )?;

    let fails: i64 = conn.query_row(
        "SELECT consecutive_fails FROM provider_health WHERE provider_name = ?1",
        params![provider_name],
        |r| r.get(0),
    )?;
    Ok(fails)
}

/// Resets consecutive failures on successful turn or health check.
pub fn reset_provider_failures(conn: &Connection, provider_name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE provider_health SET consecutive_fails = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE provider_name = ?1",
        params![provider_name],
    )?;
    Ok(())
}

/// Recovers a melted provider back to HEALTHY / READY (docs/06 §6.3).
pub fn recover_provider_health(
    conn: &Connection,
    provider_name: &str,
    new_status: Option<&str>,
) -> rusqlite::Result<()> {
    let status = new_status.unwrap_or("READY");
    conn.execute(
        "UPDATE provider_health SET
            status = ?2,
            melt_reason = NULL,
            melted_until = NULL,
            consecutive_fails = 0,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE provider_name = ?1",
        params![provider_name, status],
    )?;
    Ok(())
}

/// Updates or transitions provider status to HALF_OPEN for probing.
pub fn set_provider_half_open(conn: &Connection, provider_name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE provider_health SET
            status = 'HALF_OPEN',
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE provider_name = ?1",
        params![provider_name],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::embedded::migrations::runner().run(&mut conn).unwrap();
        conn
    }

    #[test]
    fn test_trip_and_recover_provider_melt() {
        let conn = in_memory_db();
        trip_provider_melt(&conn, "CLAUDE_CODE", MeltReason::QuotaHard, Some("2026-10-06T12:00:00Z")).unwrap();

        let health = get_provider_health(&conn, "CLAUDE_CODE").unwrap().expect("found");
        assert_eq!(health.status, "MELTED");
        assert_eq!(health.melt_reason, Some(MeltReason::QuotaHard));
        assert_eq!(health.melted_until.as_deref(), Some("2026-10-06T12:00:00Z"));

        // Half open
        set_provider_half_open(&conn, "CLAUDE_CODE").unwrap();
        let h2 = get_provider_health(&conn, "CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(h2.status, "HALF_OPEN");

        // Recover
        recover_provider_health(&conn, "CLAUDE_CODE", Some("READY")).unwrap();
        let h3 = get_provider_health(&conn, "CLAUDE_CODE").unwrap().unwrap();
        assert_eq!(h3.status, "READY");
        assert_eq!(h3.melt_reason, None);
        assert_eq!(h3.melted_until, None);
        assert_eq!(h3.consecutive_fails, 0);
    }

    #[test]
    fn test_record_failures_and_reset() {
        let conn = in_memory_db();
        let f1 = record_provider_failure(&conn, "CODEX_CLI").unwrap();
        assert_eq!(f1, 1);
        let f2 = record_provider_failure(&conn, "CODEX_CLI").unwrap();
        assert_eq!(f2, 2);
        let f3 = record_provider_failure(&conn, "CODEX_CLI").unwrap();
        assert_eq!(f3, 3);

        reset_provider_failures(&conn, "CODEX_CLI").unwrap();
        let h = get_provider_health(&conn, "CODEX_CLI").unwrap().unwrap();
        assert_eq!(h.consecutive_fails, 0);
    }
}
