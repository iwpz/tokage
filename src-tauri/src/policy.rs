//! Command lists exposed to the frontend (docs/11 §3, P2.m1): the pack's lists merged with
//! the custom pack, local `command_rules`, and classification of a command line.

use serde::{Deserialize, Serialize};
use tokage_core::command_policy::{Context, Level, PackPolicy, Policy, Rule, RuleSource, Verdict};
use tokage_storage::Storage;

const BUILTIN: &str = include_str!("../resources/knowledge/builtin.json");

fn pack_policy(pack: &serde_json::Value) -> Option<PackPolicy> {
    serde_json::from_value(pack.get("command_policy")?.clone()).ok()
}

/// Built-in lists with the custom pack's additions.
pub fn effective_pack(db: &Storage) -> Result<PackPolicy, String> {
    let builtin: serde_json::Value = serde_json::from_str(BUILTIN).map_err(|e| e.to_string())?;
    let base = pack_policy(&builtin).ok_or("builtin pack has no command_policy")?;
    let custom = db.load()?.custom_pack.and_then(|c| pack_policy(&c.pack));
    Ok(base.merged(custom.as_ref()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPolicyOut {
    pub pack: PackPolicy,
    pub rules: Vec<Rule>,
}

pub fn get(db: &Storage) -> Result<CommandPolicyOut, String> {
    Ok(CommandPolicyOut {
        pack: effective_pack(db)?,
        rules: db.command_rules()?,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum RuleChange {
    Upsert {
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        provider: Option<String>,
        pattern: String,
        level: Level,
    },
    Delete {
        id: i64,
    },
}

/// Applies one change. Additions are checked against the pack first: nothing the pack
/// forbids can be allowed. Deleting only removes the user's own rows.
pub async fn update(db: &Storage, change: RuleChange) -> Result<Vec<Rule>, String> {
    match change {
        RuleChange::Upsert {
            project_id,
            provider,
            pattern,
            level,
        } => {
            let rule = Rule {
                id: None,
                project_id,
                provider,
                pattern: pattern.split_whitespace().collect::<Vec<_>>().join(" "),
                level,
                source: RuleSource::User,
            };
            Policy::new(&effective_pack(db)?, &[]).validate_rule(&rule)?;
            db.upsert_command_rule(rule).await?;
        }
        RuleChange::Delete { id } => db.delete_command_rule(id).await?,
    }
    db.command_rules()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyRequest {
    pub command: String,
    pub cwd: String,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub write_scope: Option<Vec<String>>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub current_branch: Option<String>,
}

pub fn classify(db: &Storage, req: ClassifyRequest, home: Option<String>, app_data: Option<String>) -> Result<Verdict, String> {
    let policy = Policy::new(&effective_pack(db)?, &db.command_rules()?);
    let mut ctx = Context::new(req.cwd);
    if !req.workspace_roots.is_empty() {
        ctx.workspace_roots = req.workspace_roots;
    }
    ctx.home = home;
    ctx.app_data = app_data;
    ctx.write_scope = req.write_scope;
    ctx.project_id = req.project_id;
    ctx.provider = req.provider;
    ctx.default_branch = req.default_branch;
    ctx.current_branch = req.current_branch;
    Ok(policy.classify(&req.command, &ctx))
}
