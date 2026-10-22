//! CLI transport: arguments, explicit user intents and JSON projections. Runtime owns decisions.
use crate::{environment, options, read_text, required, state::Store};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tokage_native::{
    chat::adapter::{adapter, Caps},
    git,
};
use tokage_runtime::{
    host, repository::Reader, Command, EffectiveRunConfig, ModelAssignment, Request, RunState,
    Snapshot, SCHEMA_VERSION,
};
type Opts = BTreeMap<String, Vec<String>>;
fn optional(o: &Opts, key: &str) -> Result<Option<String>, String> {
    o.contains_key(key).then(|| required(o, key)).transpose()
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn summary(s: &Snapshot) -> Result<Value, String> {
    let mut run = serde_json::to_value(s).map_err(error)?;
    let status = match s.state {
        RunState::Stopped => "CANCELLED".into(),
        other => format!("{other:?}")
            .chars()
            .enumerate()
            .fold(String::new(), |mut out, (i, c)| {
                if i > 0 && c.is_uppercase() {
                    out.push('_');
                }
                out.push(c.to_ascii_uppercase());
                out
            }),
    };
    run["id"] = json!(s.run_id);
    run["status"] = json!(status);
    run["target"] = json!({"head":s.config.target_identity,"reference":s.config.target_reference});
    run["integration"] = json!(s.work.as_ref().and_then(|w| w.integration.as_ref()));
    run["accepted"] = json!(s
        .delivery
        .as_ref()
        .filter(|d| d.confirmation_digest.is_some())
        .and_then(|d| d.candidate.as_ref()));
    run["sequence"] = json!(s.execution.as_ref().map_or(0, |e| e.sequence));
    let completed: BTreeMap<_, _> = s
        .execution
        .iter()
        .flat_map(|e| e.tasks.iter())
        .filter(|(_, t)| t.state == tokage_orchestration::TaskState::Done)
        .filter_map(|(id, t)| {
            Some((
                id,
                s.work
                    .as_ref()?
                    .attempts
                    .get(t.attempts.last()?)?
                    .commit
                    .clone()?,
            ))
        })
        .collect();
    run["completedTasks"] = json!(completed);
    let reason = s
        .delivery
        .as_ref()
        .and_then(|d| {
            d.error
                .clone()
                .or_else(|| d.intent.as_ref().and_then(|i| i.error.clone()))
        })
        .or_else(|| {
            s.execution
                .iter()
                .flat_map(|e| e.tasks.values())
                .find_map(|t| t.error.clone())
                .or_else(|| s.turns.values().find_map(|t| t.error.clone()))
                .or_else(|| s.planning_attempt.as_ref().and_then(|p| p.error.clone()))
        });
    run["error"] = json!(reason);
    Ok(
        json!({"run":run,"planSha":s.plan.as_ref().map(|p|&p.confirmation_digest),"deliveryDigest":s.delivery.as_ref().and_then(|d|d.confirmation_digest.as_ref())}),
    )
}
fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        Err("Invalid run ID".into())
    } else {
        Ok(())
    }
}
async fn send(rt: &host::NativeRuntime, command: Command) -> Result<(), String> {
    let s = rt.snapshot().await.map_err(error)?.ok_or("Run not found")?;
    rt.submit(&host::request(&s, command))
        .await
        .map_err(error)?;
    Ok(())
}
async fn settled(rt: &host::NativeRuntime, dir: &Path) -> Result<Snapshot, String> {
    // This loop formats durable output only; host::wait owns lifecycle observation.
    let id = rt
        .snapshot()
        .await
        .map_err(error)?
        .ok_or("Run not found")?
        .run_id;
    let wait = host::wait(rt, dir);
    tokio::pin!(wait);
    let mut cursors = BTreeMap::<String, u64>::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        tokio::select! {
            result=&mut wait=>{stream(dir,&id,&mut cursors).await?;return result.map_err(error);},
            _=tokio::signal::ctrl_c()=>{host::request_cancel(dir,&id).map_err(error)?;},
            _=interval.tick()=>stream(dir,&id,&mut cursors).await?,
        }
    }
}
async fn stream(dir: &Path, id: &str, cursors: &mut BTreeMap<String, u64>) -> Result<(), String> {
    let mut reader = Reader::open(dir).map_err(error)?;
    let s = reader.snapshot(id).map_err(error)?.ok_or("Run not found")?;
    for t in s.turns.values() {
        let cursor = cursors.entry(t.id.clone()).or_default();
        for e in reader.transcript(id, &t.id, *cursor, 1000).map_err(error)? {
            eprintln!(
                "{}",
                json!({"type":"turnEvent","runId":s.run_id,"turnId":t.id,"sequence":e.sequence,"event":e.event})
            );
            *cursor = e.sequence;
        }
    }
    Ok(())
}
pub async fn dispatch(
    action: &str,
    raw: &[String],
    data: Option<PathBuf>,
) -> Result<Value, String> {
    let store = Store::new(data)?;
    let (values, flags): (&[&str], &[&str]) = match action {
        "create" => (
            &[
                "--repo",
                "--requirement-file",
                "--provider",
                "--model",
                "--provider-path",
            ],
            &["--co-author"],
        ),
        "list" => (&[], &[]),
        "submit" => (&["--file"], &[]),
        "legacy-show" => (&["--id"], &[]),
        "show" | "execute" | "cancel" | "resume" | "review" => (&["--id"], &[]),
        "events" => (&["--id", "--after", "--limit"], &[]),
        "transcript" => (&["--id", "--turn", "--after", "--limit"], &[]),
        "plan" => (&["--id"], &["--trust-workspace"]),
        "inspect-task" => (&["--id", "--task"], &[]),
        "takeover" => (
            &["--id", "--task", "--digest", "--plan-sha", "--report-file"],
            &["--confirm"],
        ),
        "import-plan" | "update-config" => (&["--id", "--file"], &[]),
        "import-history" => (&["--id", "--file", "--backup"], &[]),
        "approve" => (
            &["--id", "--plan-sha", "--confirm-command"],
            &["--allow-writes", "--trust-workspace"],
        ),
        "retry" => (&["--id"], &["--confirm"]),
        "merge" => (&["--id", "--source", "--target"], &["--confirm"]),
        "export-patch" => (&["--id", "--digest", "--output"], &["--confirm"]),
        "keep-branch" => (&["--id", "--digest"], &["--confirm"]),
        "resolve-approval" => (
            &["--id", "--turn", "--approval", "--digest", "--decision"],
            &[],
        ),
        _ => return Err("Unknown run subcommand".into()),
    };
    let o = options(raw, values, flags)?;
    for key in values.iter().filter(|k| **k != "--confirm-command") {
        if o.contains_key(*key) {
            required(&o, key)?;
        }
    }
    let dir = store.root.join("runtime-v1");
    if action == "submit" {
        let req: Request = serde_json::from_str(&read_text(Path::new(&required(&o, "--file")?))?)
            .map_err(error)?;
        check_id(&req.run_id)?;
        let rt = host::open(&dir, &req.run_id, environment()).map_err(error)?;
        let receipt = rt.submit(&req).await.map_err(error)?;
        let snapshot = settled(&rt, &dir).await?;
        return Ok(json!({"receipt":receipt,"snapshot":snapshot}));
    }
    if action == "legacy-show" {
        let id = required(&o, "--id")?;
        check_id(&id)?;
        let path = store.root.join("runs").join(&id).join("run.json");
        if std::fs::symlink_metadata(&path)
            .map_err(error)?
            .file_type()
            .is_symlink()
        {
            return Err("Refusing symlinked history".into());
        }
        let legacy: Value = serde_json::from_str(&read_text(&path)?).map_err(error)?;
        if legacy["schemaVersion"] != 1 || legacy["id"] != id {
            return Err("Unknown legacy schema or identity".into());
        }
        return Ok(json!({"historical":true,"executable":false,"original":legacy}));
    }
    if action == "create" {
        let repo = PathBuf::from(required(&o, "--repo")?)
            .canonicalize()
            .map_err(error)?;
        let requirement = read_text(Path::new(&required(&o, "--requirement-file")?))?;
        let provider = required(&o, "--provider")?.to_ascii_uppercase();
        let provider_id: tokage_core::ProviderId = serde_json::from_value(json!(provider))
            .map_err(|_| "Use a Provider ID such as CODEX_CLI")?;
        let caps = adapter(provider_id)
            .ok_or("Provider has no headless adapter")?
            .meta()
            .caps;
        if !caps.supports_full_auto() || !caps.has(Caps::READ_ONLY_MODE) || cfg!(windows) {
            return Err("Full workflow requires supported sandbox and read-only adapter".into());
        }
        let model = required(&o, "--model")?;
        if model.trim().is_empty() || requirement.trim().is_empty() {
            return Err("Model and requirement must be nonempty".into());
        }
        let env = environment();
        if Path::new(
            git::run(&env, &repo, &["rev-parse", "--show-toplevel"], &[], 10)
                .await?
                .trim(),
        )
        .canonicalize()
        .map_err(error)?
            != repo
        {
            return Err("--repo must be repository root".into());
        }
        if !git::is_working_tree_clean(&env, &repo).await? {
            return Err("Repository must be clean".into());
        }
        let mut ancestor = store.root.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or("Invalid data directory")?;
        }
        if ancestor.canonicalize().map_err(error)?.starts_with(&repo) {
            return Err("CLI data directory must be outside repository".into());
        }
        let target = git::checkout_identity(&env, &repo).await?;
        if target.reference == "HEAD" {
            return Err("Detached target unsupported".into());
        }
        let selected = ModelAssignment {
            provider,
            requested_model: model,
            provider_path: optional(&o, "--provider-path")?,
            capabilities: vec!["STRUCTURED_OUTPUT".into(), "READ_ONLY_MODE".into()],
            user_locked: true,
            model_params: None,
        };
        let config = EffectiveRunConfig {
            source: "cli-explicit-v1".into(),
            policy_version: "runtime-v1".into(),
            workspace: repo.to_string_lossy().into(),
            target_identity: target.head,
            target_reference: Some(target.reference),
            scope: vec!["**".into()],
            models: BTreeMap::from([
                ("executor".into(), selected.clone()),
                ("planner".into(), selected.clone()),
                ("reviewer".into(), selected),
            ]),
            command_policy_hash: tokage_runtime::digest(
                &tokage_matching::pack::builtin_knowledge_pack().command_policy,
            )
            .map_err(error)?,
            trust_workspace: false,
            permission_mode: "full-auto".into(),
            concurrency: 1,
            provider_concurrency: BTreeMap::new(),
            budget_usd: None,
            block_unknown_cost: false,
            timeout_seconds: 600,
            repair_policy: "manual".into(),
            context_policy: "off".into(),
            gate_env_allow: vec![],
            confirmed_commands: vec![],
            custom_pack_hashes: vec![],
            co_author: o.contains_key("--co-author"),
            planning_policy: None,
        };
        let id = host::identifier();
        let rt = host::open(&dir, &id, env).map_err(error)?;
        rt.submit(&Request {
            schema_version: SCHEMA_VERSION,
            command_id: host::identifier(),
            run_id: id,
            expected_revision: 0,
            command: Command::CreateRun {
                requirement,
                config,
            },
        })
        .await
        .map_err(error)?;
        return summary(&rt.snapshot().await.map_err(error)?.ok_or("Run not found")?);
    }
    if action == "list" {
        let runs = if dir.join("tokage.db").exists() {
            Reader::open(&dir)
                .map_err(error)?
                .list()
                .map_err(error)?
                .iter()
                .map(summary)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };
        return Ok(
            json!({"runs":runs.into_iter().map(|v|v["run"].clone()).collect::<Vec<_>>(),"legacyDirectory":store.root.join("runs"),"legacyPolicy":"read-only; explicit import-history archives without executing"}),
        );
    }
    let id = required(&o, "--id")?;
    check_id(&id)?;
    if action == "import-history" {
        let text = read_text(Path::new(&required(&o, "--file")?))?;
        let repo = tokage_runtime::repository::SqliteRepository::open(&dir, &id).map_err(error)?;
        let hash = repo
            .import_history("cli-v1", &id, &text, Path::new(&required(&o, "--backup")?))
            .await
            .map_err(error)?;
        return Ok(json!({"historical":true,"id":id,"contentHash":hash,"executable":false}));
    }
    if !dir.join("tokage.db").exists() {
        return Err(
            "Legacy JSON is read-only; use explicit import-history (never executes)".into(),
        );
    }
    let mut reader = Reader::open(&dir).map_err(error)?;
    let snapshot = reader
        .snapshot(&id)
        .map_err(error)?
        .ok_or("Run not found; legacy JSON cannot execute")?;
    if action == "show" {
        return summary(&snapshot);
    }
    if action == "events" || action == "transcript" {
        let after = optional(&o, "--after")?
            .unwrap_or("0".into())
            .parse::<u64>()
            .map_err(error)?;
        let limit = optional(&o, "--limit")?
            .unwrap_or("1000".into())
            .parse::<u32>()
            .map_err(error)?;
        return if action == "events" {
            Ok(json!({"events":reader.events(&id,after,limit).map_err(error)?}))
        } else {
            Ok(
                json!({"events":reader.transcript(&id,&required(&o,"--turn")?,after,limit).map_err(error)?}),
            )
        };
    }
    drop(reader);
    if action == "cancel" {
        let completed = match host::open(&dir, &id, environment()) {
            Ok(rt) => {
                send(&rt, Command::CancelRun).await?;
                host::wait(&rt, &dir).await.map_err(error)?.state == RunState::Stopped
            }
            Err(tokage_runtime::Error::OwnerConflict) => {
                host::request_cancel(&dir, &id).map_err(error)?;
                false
            }
            Err(e) => return Err(error(e)),
        };
        return Ok(json!({"id":id,"cancellationRequested":true,"completionConfirmed":completed}));
    }
    let rt = host::open(&dir, &id, environment()).map_err(error)?;
    if action == "inspect-task" {
        return serde_json::to_value(
            rt.inspect_task(&required(&o, "--task")?)
                .await
                .map_err(error)?,
        )
        .map_err(error);
    }
    let command = match action {
        "takeover" => {
            if !o.contains_key("--confirm") {
                return Err("Takeover requires --confirm".into());
            }
            Command::AcceptTakeover {
                task_id: required(&o, "--task")?,
                source_digest: required(&o, "--digest")?,
                confirmation_digest: required(&o, "--plan-sha")?,
                report: read_text(Path::new(&required(&o, "--report-file")?))?,
            }
        }
        "plan" => {
            if !o.contains_key("--trust-workspace") {
                return Err("Planning requires --trust-workspace".into());
            }
            if !snapshot.config.trust_workspace {
                let mut config = snapshot.config.clone();
                config.trust_workspace = true;
                send(&rt, Command::UpdateConfig { config }).await?;
            }
            Command::GeneratePlan
        }
        "update-config" => Command::UpdateConfig {
            config: serde_json::from_str(&read_text(Path::new(&required(&o, "--file")?))?)
                .map_err(error)?,
        },
        "import-plan" => Command::ImportPlan {
            text: read_text(Path::new(&required(&o, "--file")?))?,
        },
        "approve" => {
            if !o.contains_key("--allow-writes") || !o.contains_key("--trust-workspace") {
                return Err("Explicit --allow-writes and --trust-workspace required".into());
            }
            Command::ConfirmPlan {
                confirmation_digest: required(&o, "--plan-sha")?,
                confirmed_commands: o.get("--confirm-command").cloned().unwrap_or_default(),
            }
        }
        "execute" => Command::StartExecution {
            confirmation_digest: snapshot
                .approved_digest
                .clone()
                .ok_or("Plan not approved")?,
        },
        "retry" => {
            if !o.contains_key("--confirm") {
                return Err("Retry requires explicit --confirm".into());
            }
            Command::PrepareRetry {
                confirmation_digest: snapshot
                    .plan
                    .as_ref()
                    .ok_or("Plan missing")?
                    .confirmation_digest
                    .clone(),
            }
        }
        "resume" => Command::ResumeRun,
        "review" => Command::ReviewDelivery,
        "merge" => {
            if !o.contains_key("--confirm") {
                return Err("Merge requires --confirm".into());
            }
            if required(&o, "--source")?
                != snapshot
                    .delivery
                    .as_ref()
                    .and_then(|d| d.candidate.as_ref())
                    .ok_or("No accepted candidate")?
                    .head
                || required(&o, "--target")? != snapshot.config.target_identity
            {
                return Err("Source or target confirmation stale".into());
            }
            Command::MergeDelivery {
                confirmation_digest: snapshot
                    .delivery
                    .as_ref()
                    .and_then(|d| d.confirmation_digest.clone())
                    .ok_or("No delivery acceptance")?,
            }
        }
        "export-patch" | "keep-branch" => {
            if !o.contains_key("--confirm") {
                return Err("Delivery requires --confirm".into());
            }
            let confirmation_digest = required(&o, "--digest")?;
            if action == "export-patch" {
                Command::ExportPatch {
                    confirmation_digest,
                    output_path: required(&o, "--output")?,
                }
            } else {
                Command::KeepBranch {
                    confirmation_digest,
                }
            }
        }
        "resolve-approval" => Command::ResolveApproval {
            decision: match required(&o, "--decision")?.as_str() {
                "allow-once" => tokage_runtime::ApprovalDecision::AllowOnce,
                "allow-session" => tokage_runtime::ApprovalDecision::AllowSession,
                "deny" => tokage_runtime::ApprovalDecision::Deny,
                _ => return Err("Use allow-once, allow-session or deny".into()),
            },
            turn_id: required(&o, "--turn")?,
            approval_id: required(&o, "--approval")?,
            confirmation_digest: required(&o, "--digest")?,
        },
        _ => return Err("Unknown run operation".into()),
    };
    send(&rt, command).await?;
    let s = settled(&rt, &dir).await?;
    if matches!(s.state, RunState::Blocked | RunState::Stopped) {
        return Err(format!(
            "Run {:?}: {}; inspect run show and events",
            s.state,
            summary(&s)?["run"]["error"]
        ));
    }
    summary(&s)
}
