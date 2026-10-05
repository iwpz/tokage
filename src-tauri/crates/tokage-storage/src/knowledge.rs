//! Knowledge base storage tables & operations (docs/08 schema.md §6, P6.m1c).

use std::collections::HashMap;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokage_matching::{KnowledgePack, SuccessModel};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelKnowledgeRow {
    pub model_id: String,
    pub display_name: String,
    pub vendor: Option<String>,
    pub roles: Vec<String>,
    pub skills: HashMap<String, f64>,
    pub price_json: String,
    pub cache_json: Option<String>,
    pub context_window: u64,
    pub effective_context: u64,
    pub speed: Option<i64>,
    pub tags: Vec<String>,
    pub description: String,
    pub meta_json: Option<String>,
    pub deprecated: bool,
    pub pack_version: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelOfferingRow {
    pub provider_name: String,
    pub model_id: String,
    pub cli_model_arg: String,
    pub context_window: Option<u64>,
    pub effective_context: Option<u64>,
    pub credit_multiplier: Option<f64>,
    pub skills_patch: Option<HashMap<String, f64>>,
    pub verified: bool,
    pub pack_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelOverrideRow {
    pub provider_name: String,
    pub model_id: String,
    pub patch_json: String,
    pub updated_at: String,
}

/// Atomically persists a full knowledge pack into the SQLite knowledge tables.
pub fn save_pack(conn: &Connection, pack: &KnowledgePack) -> rusqlite::Result<()> {

    // 1. provider_profiles
    for (pid, prov) in &pack.providers {
        let profile_json = serde_json::to_string(prov).unwrap_or_else(|_| "{}".into());
        conn.execute(
            "INSERT INTO provider_profiles (provider_name, profile_json, pack_version)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(provider_name) DO UPDATE SET
                profile_json = excluded.profile_json,
                pack_version = excluded.pack_version",
            params![pid, profile_json, pack.pack_version],
        )?;
    }

    // 2. model_knowledge_base
    for (mid, model) in &pack.models {
        let roles_json = serde_json::to_string(&model.roles).unwrap_or_else(|_| "[]".into());
        let skills_json = serde_json::to_string(&model.skills).unwrap_or_else(|_| "{}".into());
        let price_json = serde_json::to_string(&serde_json::json!({
            "price_per_mtok": model.price_per_mtok,
            "cache_rates": model.cache_rates,
            "long_context_pricing": model.long_context_pricing,
            "price_schedule": model.price_schedule,
            "price_estimated": model.price_estimated,
        }))
        .unwrap_or_else(|_| "{}".into());
        let cache_json = model
            .cache_rates
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default());
        let tags_json = serde_json::to_string(&model.tags).unwrap_or_else(|_| "[]".into());
        let meta_json = serde_json::to_string(&serde_json::json!({
            "released": model.released,
            "max_output": model.max_output,
            "benchmarks": model.benchmarks,
            "skills_estimated": model.skills_estimated,
        }))
        .ok();
        let speed_val = model.speed.map(|s| s.round() as i64);

        conn.execute(
            "INSERT INTO model_knowledge_base (
                model_id, display_name, vendor, roles, skills_json, price_json,
                cache_json, context_window, effective_context, speed, tags,
                description, meta_json, deprecated, pack_version, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, (strftime('%Y-%m-%dT%H:%M:%fZ','now')))
             ON CONFLICT(model_id) DO UPDATE SET
                display_name = excluded.display_name,
                vendor = excluded.vendor,
                roles = excluded.roles,
                skills_json = excluded.skills_json,
                price_json = excluded.price_json,
                cache_json = excluded.cache_json,
                context_window = excluded.context_window,
                effective_context = excluded.effective_context,
                speed = excluded.speed,
                tags = excluded.tags,
                description = excluded.description,
                meta_json = excluded.meta_json,
                deprecated = excluded.deprecated,
                pack_version = excluded.pack_version,
                updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![
                mid,
                model.display_name,
                model.vendor,
                roles_json,
                skills_json,
                price_json,
                cache_json,
                model.context_window as i64,
                model.effective_context as i64,
                speed_val,
                tags_json,
                model.description.as_deref().unwrap_or(""),
                meta_json,
                if model.deprecated.unwrap_or(false) { 1 } else { 0 },
                pack.pack_version
            ],
        )?;
    }

    // 3. model_offerings
    for (pid, prov) in &pack.providers {
        for offering in &prov.models {
            let patch_json = offering
                .skills
                .as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_default());
            conn.execute(
                "INSERT INTO model_offerings (
                    provider_name, model_id, cli_model_arg, context_window,
                    effective_context, credit_multiplier, skills_patch_json,
                    verified, pack_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(provider_name, model_id) DO UPDATE SET
                    cli_model_arg = excluded.cli_model_arg,
                    context_window = excluded.context_window,
                    effective_context = excluded.effective_context,
                    credit_multiplier = excluded.credit_multiplier,
                    skills_patch_json = excluded.skills_patch_json,
                    verified = excluded.verified,
                    pack_version = excluded.pack_version",
                params![
                    pid,
                    offering.model,
                    offering.cli_model_arg,
                    offering.context_window.map(|c| c as i64),
                    offering.effective_context.map(|c| c as i64),
                    offering.credit_multiplier,
                    patch_json,
                    if offering.cli_model_arg_verified { 1 } else { 0 },
                    pack.pack_version
                ],
            )?;
        }
    }

    // 4. knowledge_blobs
    let blobs: [(&str, String); 6] = [
        (
            "success_model",
            serde_json::to_string(&pack.success_model).unwrap_or_else(|_| "{}".into()),
        ),
        (
            "domains",
            serde_json::to_string(&pack.domains).unwrap_or_else(|_| "[]".into()),
        ),
        (
            "usage_profiles",
            serde_json::to_string(&pack.usage_profiles).unwrap_or_else(|_| "{}".into()),
        ),
        (
            "triage_rules",
            serde_json::to_string(&pack.triage_rules).unwrap_or_else(|_| "[]".into()),
        ),
        (
            "domain_keywords",
            serde_json::to_string(&pack.domain_keywords).unwrap_or_else(|_| "{}".into()),
        ),
        (
            "usage_patterns",
            serde_json::to_string(&pack.usage_patterns).unwrap_or_else(|_| "{}".into()),
        ),
    ];

    for (k, val) in blobs {
        conn.execute(
            "INSERT INTO knowledge_blobs (key, value_json, pack_version)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                pack_version = excluded.pack_version",
            params![k, val, pack.pack_version],
        )?;
    }

    Ok(())
}

pub fn list_models(conn: &Connection) -> rusqlite::Result<Vec<ModelKnowledgeRow>> {
    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, vendor, roles, skills_json, price_json,
                cache_json, context_window, effective_context, speed, tags,
                description, meta_json, deprecated, pack_version, updated_at
         FROM model_knowledge_base
         ORDER BY model_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let roles_raw: String = row.get(3)?;
        let skills_raw: String = row.get(4)?;
        let tags_raw: String = row.get(10)?;
        let dep: i64 = row.get(13)?;

        Ok(ModelKnowledgeRow {
            model_id: row.get(0)?,
            display_name: row.get(1)?,
            vendor: row.get(2)?,
            roles: serde_json::from_str(&roles_raw).unwrap_or_default(),
            skills: serde_json::from_str(&skills_raw).unwrap_or_default(),
            price_json: row.get(5)?,
            cache_json: row.get(6)?,
            context_window: row.get::<_, i64>(7)? as u64,
            effective_context: row.get::<_, i64>(8)? as u64,
            speed: row.get(9)?,
            tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
            description: row.get(11)?,
            meta_json: row.get(12)?,
            deprecated: dep != 0,
            pack_version: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;

    rows.collect()
}

pub fn list_offerings(conn: &Connection) -> rusqlite::Result<Vec<ModelOfferingRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, model_id, cli_model_arg, context_window,
                effective_context, credit_multiplier, skills_patch_json,
                verified, pack_version
         FROM model_offerings
         ORDER BY provider_name ASC, model_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let patch_raw: Option<String> = row.get(6)?;
        let ver: i64 = row.get(7)?;
        let ctx: Option<i64> = row.get(3)?;
        let eff: Option<i64> = row.get(4)?;

        Ok(ModelOfferingRow {
            provider_name: row.get(0)?,
            model_id: row.get(1)?,
            cli_model_arg: row.get(2)?,
            context_window: ctx.map(|c| c as u64),
            effective_context: eff.map(|c| c as u64),
            credit_multiplier: row.get(5)?,
            skills_patch: patch_raw.and_then(|p| serde_json::from_str(&p).ok()),
            verified: ver != 0,
            pack_version: row.get(8)?,
        })
    })?;

    rows.collect()
}

pub fn get_model_skills(
    conn: &Connection,
    model_id: &str,
) -> rusqlite::Result<Option<HashMap<String, f64>>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT skills_json FROM model_knowledge_base WHERE model_id = ?1",
            params![model_id],
            |row| row.get(0),
        )
        .optional()?;

    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn get_success_model(conn: &Connection) -> rusqlite::Result<Option<SuccessModel>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM knowledge_blobs WHERE key = 'success_model'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn get_knowledge_blob(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value_json FROM knowledge_blobs WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

pub fn upsert_override(
    conn: &Connection,
    provider_name: &str,
    model_id: &str,
    patch_json: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO model_overrides (provider_name, model_id, patch_json, updated_at)
         VALUES (?1, ?2, ?3, (strftime('%Y-%m-%dT%H:%M:%fZ','now')))
         ON CONFLICT(provider_name, model_id) DO UPDATE SET
            patch_json = excluded.patch_json,
            updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![provider_name, model_id, patch_json],
    )?;
    Ok(())
}

pub fn list_overrides(conn: &Connection) -> rusqlite::Result<Vec<ModelOverrideRow>> {
    let mut stmt = conn.prepare(
        "SELECT provider_name, model_id, patch_json, updated_at
         FROM model_overrides
         ORDER BY provider_name ASC, model_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ModelOverrideRow {
            provider_name: row.get(0)?,
            model_id: row.get(1)?,
            patch_json: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_override(
    conn: &Connection,
    provider_name: &str,
    model_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM model_overrides WHERE provider_name = ?1 AND model_id = ?2",
        params![provider_name, model_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_matching::builtin_knowledge_pack;

    #[test]
    fn test_save_and_query_knowledge_pack() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::embedded::migrations::runner()
            .run(&mut conn)
            .unwrap();

        let pack = builtin_knowledge_pack();
        save_pack(&conn, pack).unwrap();

        let models = list_models(&conn).unwrap();
        assert!(!models.is_empty());
        let opus = models.iter().find(|m| m.model_id == "claude-opus-5-5").unwrap();
        assert_eq!(opus.display_name, "Claude Opus 5.5");
        assert_eq!(opus.skills.get("planning"), Some(&5.0));

        let offerings = list_offerings(&conn).unwrap();
        assert!(!offerings.is_empty());
        assert!(offerings.iter().any(|o| o.provider_name == "CLAUDE_CODE" && o.model_id == "claude-opus-5-5"));

        let succ = get_success_model(&conn).unwrap().expect("success_model blob present");
        assert_eq!(succ.a, 1.6);
        assert_eq!(succ.req["1"]["S"], 2.0);

        let domains_blob = get_knowledge_blob(&conn, "domains").unwrap().expect("domains present");
        assert!(domains_blob.contains("planning"));

        // Test overrides
        upsert_override(&conn, "*", "claude-opus-5-5", r#"{"skills":{"testing":5}}"#).unwrap();
        let ovs = list_overrides(&conn).unwrap();
        assert_eq!(ovs.len(), 1);
        assert_eq!(ovs[0].model_id, "claude-opus-5-5");

        delete_override(&conn, "*", "claude-opus-5-5").unwrap();
        assert!(list_overrides(&conn).unwrap().is_empty());
    }
}
