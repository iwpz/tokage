mod runs;
pub mod state;
mod turn;
// Public headless product entry. Shared engines stay independent of CLI and Tauri.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tokage_native::{env::ShellEnv, git};

pub const HELP: &str = "Tokage CLI
Usage: tokage [--json] [--data-dir PATH] COMMAND

  providers list                        Detect installed providers (no inference)
  repo inspect --repo PATH              Inspect Git checkout and project digest
  plan validate --file PATH             Parse and validate ordinary plan Markdown
  run create | list | show | events
  run plan | import-plan | approve
  --help | --version

Run commands are documented in docs/cli.md as they become available.
No model requests are made by help, detection, inspection or validation.";

#[derive(Debug)]
pub struct Args {
    pub command: Vec<String>,
    pub data_dir: Option<PathBuf>,
}
impl Args {
    pub fn parse(raw: Vec<String>) -> Result<Self, String> {
        let mut command = vec![];
        let mut data_dir = None;
        let mut machine = false;
        let mut it = raw.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--json" => {
                    if machine {
                        return Err("Duplicate --json".into());
                    }
                    machine = true;
                }
                "--data-dir" => {
                    if data_dir.is_some() {
                        return Err("Duplicate --data-dir".into());
                    }
                    data_dir = Some(PathBuf::from(next_value(&mut it, "--data-dir")?));
                }
                _ => command.push(arg),
            }
        }
        Ok(Self { command, data_dir })
    }
}
fn next_value(it: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    it.next()
        .filter(|s| !s.starts_with("--") && !s.is_empty())
        .ok_or_else(|| format!("Missing value for {name}"))
}

/// Parse each command against its own contract before any side effects.
pub fn options(
    raw: &[String],
    values: &[&str],
    flags: &[&str],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut result = BTreeMap::<String, Vec<String>>::new();
    let mut it = raw.iter().cloned();
    while let Some(key) = it.next() {
        if values.contains(&key.as_str()) {
            let value = next_value(&mut it, &key)?;
            result.entry(key).or_default().push(value);
        } else if flags.contains(&key.as_str()) {
            if result.contains_key(&key) {
                return Err(format!("Duplicate {key}"));
            }
            result.insert(key, vec![]);
        } else {
            return Err(format!("Unknown argument: {key}"));
        }
    }
    Ok(result)
}
pub fn required(opts: &BTreeMap<String, Vec<String>>, key: &str) -> Result<String, String> {
    match opts.get(key) {
        Some(v) if v.len() == 1 => Ok(v[0].clone()),
        _ => Err(format!("Exactly one {key} is required")),
    }
}
pub fn environment() -> ShellEnv {
    ShellEnv {
        vars: std::env::vars().collect(),
    }
}

pub fn checked_plan(markdown: &str) -> Result<tokage_planning::ValidationResult, String> {
    let mut parsed =
        tokage_planning::parse_plan(markdown, &tokage_planning::PlanParseOptions::default())
            .map_err(|e| e.to_string())?;
    // Imported or model-authored fields cannot grant CLI command consent.
    for task in &mut parsed.tasks {
        task.acceptance.confirmed_commands.clear();
    }
    Ok(tokage_planning::validate_plan(
        parsed,
        &tokage_planning::PlanValidationOptions {
            auto_serialize_conflicts: false,
            gate_allowlist: tokage_matching::pack::builtin_knowledge_pack()
                .gate_allowlist
                .clone(),
            ..Default::default()
        },
    ))
}

pub async fn dispatch(args: Args) -> Result<Value, String> {
    let cmd = &args.command;
    if cmd.len() < 2 {
        return Err("Expected command and subcommand; use --help".into());
    }
    match (cmd[0].as_str(), cmd[1].as_str()) {
        ("providers", "list") => {
            options(&cmd[2..], &[], &[])?;
            let entries =
                tokage_native::providers::detect_all(&environment(), &Default::default()).await;
            // Expose capabilities and model IDs, never account details or environment values.
            Ok(
                json!({"providers":entries.iter().map(|p|json!({"id":p.id,"status":p.status,"version":p.version,"path":p.path,"models":p.models,"adapter":p.adapter})).collect::<Vec<_>>()}),
            )
        }
        ("repo", "inspect") => {
            let opts = options(&cmd[2..], &["--repo"], &[])?;
            let path = PathBuf::from(required(&opts, "--repo")?)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let info = git::checkout_identity(&environment(), &path).await?;
            let digest = tokage_planning::RepoDigestBuilder::new(&path).build().await;
            Ok(json!({"checkout":info,"digest":digest}))
        }
        ("plan", "validate") => {
            let opts = options(&cmd[2..], &["--file"], &[])?;
            let text =
                std::fs::read_to_string(required(&opts, "--file")?).map_err(|e| e.to_string())?;
            let result = checked_plan(&text)?;
            if !result.is_valid {
                return Err(serde_json::to_string(&result).map_err(|e| e.to_string())?);
            }
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        ("run", action) => runs::dispatch(action, &cmd[2..], args.data_dir).await,
        _ => Err("Unknown command; use --help".into()),
    }
}

pub fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}
