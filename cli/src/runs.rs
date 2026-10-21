use crate::{
    checked_plan, environment, options, read_text, required,
    state::{self, Approval, Run, Status, Store},
    turn,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tokage_core::{ProviderId, Role};
use tokage_native::{
    chat::adapter::{adapter, Caps},
    git,
};

fn optional(opts: &BTreeMap<String, Vec<String>>, key: &str) -> Result<Option<String>, String> {
    if opts.contains_key(key) {
        required(opts, key).map(Some)
    } else {
        Ok(None)
    }
}
fn summary(run: &Run) -> Result<Value, String> {
    Ok(
        json!({"run":run,"planSha":if run.plan.is_some(){Some(run.plan_sha()?)}else{None},"next":match run.status{Status::Draft=>"run plan or run import-plan",Status::AwaitingApproval=>"run approve",Status::Approved=>"run execute",Status::AwaitingMerge=>"run merge",Status::Blocked|Status::Cancelled=>"inspect events; run retry explicitly",_=>"run show"}}),
    )
}

pub async fn dispatch(
    action: &str,
    raw: &[String],
    data: Option<PathBuf>,
) -> Result<Value, String> {
    let store = Store::new(data)?;
    if action == "create" {
        let o = options(
            raw,
            &[
                "--repo",
                "--requirement-file",
                "--provider",
                "--model",
                "--provider-path",
            ],
            &["--co-author"],
        )?;
        let repo = PathBuf::from(required(&o, "--repo")?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let requirement = read_text(Path::new(&required(&o, "--requirement-file")?))?;
        let provider_name = required(&o, "--provider")?;
        let provider: ProviderId =
            serde_json::from_value(json!(provider_name.to_ascii_uppercase()))
                .map_err(|_| "Use a Provider ID such as CODEX_CLI".to_string())?;
        let model = required(&o, "--model")?;
        let provider_path = optional(&o, "--provider-path")?;
        if model.trim().is_empty() || requirement.trim().is_empty() {
            return Err("Model and requirement must be nonempty".into());
        }
        let caps = adapter(provider)
            .ok_or("Provider has no headless adapter")?
            .meta()
            .caps;
        if !caps.supports_full_auto() || !caps.has(Caps::READ_ONLY_MODE) {
            return Err(
                "Full workflow requires a workspace sandbox and read-only planner/reviewer support"
                    .into(),
            );
        }
        if cfg!(windows) {
            return Err("Full-auto execution is not supported on Windows v1".into());
        }
        let env = environment();
        let root = git::run(&env, &repo, &["rev-parse", "--show-toplevel"], &[], 10).await?;
        if Path::new(root.trim())
            .canonicalize()
            .map_err(|e| e.to_string())?
            != repo
        {
            return Err("--repo must be the repository root".into());
        }
        if !git::is_working_tree_clean(&env, &repo).await? {
            return Err("Repository must be clean before registering a run".into());
        }
        // Canonicalize the nearest existing ancestor to catch data-dir symlinks into a repo.
        let mut ancestor = store.root.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or("Invalid data directory")?;
        }
        if ancestor
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&repo)
        {
            return Err("CLI data directory must be outside the repository".into());
        }
        let target = git::checkout_identity(&env, &repo).await?;
        if target.reference == "HEAD" {
            return Err("Detached checkout cannot be a merge target".into());
        }
        let run = Run {
            schema_version: 1,
            id: state::nonce(),
            repo,
            target,
            provider,
            model,
            provider_path,
            requirement,
            status: Status::Draft,
            plan: None,
            approval: None,
            integration: None,
            integration_head: None,
            completed_tasks: BTreeMap::new(),
            task_reports: BTreeMap::new(),
            sequence: 0,
            error: None,
            accepted: None,
            review: None,
            co_author: o.contains_key("--co-author"),
        };
        store.initialize(&run.id)?;
        let _lease = store.lease(&run.id)?;
        store.save(&run)?;
        store.event(&run.id,&json!({"type":"created","id":run.id,"provider":run.provider,"model":run.model,"target":run.target}))?;
        return summary(&run);
    }
    if action == "list" {
        options(raw, &[], &[])?;
        let mut records = vec![];
        let dir = store.root.join("runs");
        if dir.exists() {
            for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    let run = store.load(&entry.file_name().to_string_lossy())?;
                    records.push(json!({"id":run.id,"status":run.status,"provider":run.provider,"model":run.model,"repo":run.repo}));
                }
            }
        }
        return Ok(json!({"runs":records}));
    }
    let (values, flags): (&[&str], &[&str]) = match action {
        "show" | "events" | "execute" | "cancel" => (&["--id"], &[]),
        "plan" => (&["--id"], &["--trust-workspace"]),
        "import-plan" => (&["--id", "--file"], &[]),
        "approve" => (
            &["--id", "--plan-sha", "--confirm-command"],
            &["--allow-writes", "--trust-workspace"],
        ),
        "retry" => (&["--id"], &["--confirm"]),
        "merge" => (&["--id", "--source", "--target"], &["--confirm"]),
        _ => return Err("Unknown run subcommand".into()),
    };
    let o = options(raw, values, flags)?;
    let id = required(&o, "--id")?;
    // Validate singular options even when a command does not require them.
    for key in values.iter().filter(|k| **k != "--confirm-command") {
        if o.contains_key(*key) {
            required(&o, key)?;
        }
    }
    if action == "show" {
        return summary(&store.load(&id)?);
    }
    if action == "events" {
        store.load(&id)?;
        let p = store.run_dir(&id)?.join("events.jsonl");
        let text = if p.exists() {
            read_text(&p)?
        } else {
            String::new()
        };
        let events: Vec<Value> = text
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("Incomplete event log: {e}"))?;
        return Ok(json!({"events":events}));
    }
    if action == "cancel" {
        let run = store.load(&id)?;
        if !matches!(run.status, Status::Planning | Status::Executing) {
            return Err("Only an active run can be cancelled".into());
        }
        state::atomic_json(
            &store.run_dir(&id)?.join("cancel.json"),
            &json!({"requested":true}),
        )?;
        return Ok(json!({"id":id,"cancellationRequested":true,"completionConfirmed":false}));
    }
    let _lease = store.lease(&id)?;
    let mut run = store.load(&id)?;
    match action {
        "import-plan" => {
            if !matches!(run.status, Status::Draft | Status::AwaitingApproval) {
                return Err("Cannot replace an approved or active plan".into());
            }
            let text = read_text(Path::new(&required(&o, "--file")?))?;
            set_plan(&mut run, &text)?;
            store.save(&run)?;
        }
        "plan" => {
            if !matches!(run.status, Status::Draft | Status::AwaitingApproval) {
                return Err("Cannot plan an approved or active run".into());
            }
            if !o.contains_key("--trust-workspace") {
                return Err("Planning requires explicit --trust-workspace".into());
            }
            let result = async {
                ensure_integration(&store, &mut run).await?;
                run.status = Status::Planning;
                run.approval = None;
                store.save(&run)?;
                let cwd = PathBuf::from(&run.integration.as_ref().unwrap().cwd);
                let digest = tokage_planning::RepoDigestBuilder::new(&cwd).build().await;
                let prompt =
                    tokage_planning::render_planning_prompt(tokage_planning::PlanningPromptArgs {
                        user_request: &run.requirement,
                        repo_digest: &digest.markdown,
                        domains: None,
                        output_max_tokens: None,
                        custom_format_spec: None,
                    });
                let env = environment();
                let before = git::scope::snapshot(&env, &cwd).await?;
                let text = turn::run_turn(
                    &store,
                    &run,
                    &env,
                    &cwd,
                    prompt,
                    Role::Planner,
                    "planning",
                    &[],
                )
                .await?;
                if git::scope::snapshot(&env, &cwd).await? != before {
                    return Err("Read-only planner modified its checkout".into());
                }
                set_plan(&mut run, &text.text)?;
                store.save(&run)
            }
            .await;
            if let Err(e) = result {
                block(&store, &mut run, &e)?;
                return Err(e);
            }
        }
        "approve" => {
            if run.status != Status::AwaitingApproval {
                return Err("Run is not awaiting plan approval".into());
            }
            let sha = required(&o, "--plan-sha")?;
            if sha != run.plan_sha()? {
                return Err("Plan digest is stale".into());
            }
            if !o.contains_key("--allow-writes") || !o.contains_key("--trust-workspace") {
                return Err("Explicit --allow-writes and --trust-workspace are required".into());
            }
            let confirmed = o.get("--confirm-command").cloned().unwrap_or_default();
            let plan = run.plan.as_ref().ok_or("Plan missing")?;
            for command in &confirmed {
                if !plan
                    .tasks
                    .iter()
                    .any(|t| t.acceptance.commands.contains(command))
                {
                    return Err(format!("Confirmation is not a plan command: {command}"));
                }
            }
            let allow = &tokage_matching::pack::builtin_knowledge_pack().gate_allowlist;
            for task in &plan.tasks {
                for command in &task.acceptance.commands {
                    if let tokage_orchestration::GateCommandPermission::Denied(reason) =
                        tokage_orchestration::check_gate_command_safety(
                            command,
                            confirmed.contains(command),
                            allow,
                        )
                    {
                        return Err(format!(
                            "Gate needs explicit command consent or is unsafe: {command}: {reason}"
                        ));
                    }
                }
            }
            run.approval = Some(Approval {
                plan_sha: sha,
                confirmed_commands: confirmed,
                allow_writes: true,
                trust_workspace: true,
            });
            run.status = Status::Approved;
            store.save(&run)?;
            store.event(
                &id,
                &json!({"type":"planApproved","planSha":run.plan_sha()?}),
            )?;
        }
        "execute" => crate::execute::execute(&store, &mut run).await?,
        _ => return Err("This run operation is not yet available".into()),
    }
    summary(&run)
}

pub fn set_plan(run: &mut Run, text: &str) -> Result<(), String> {
    let result = checked_plan(text)?;
    if result.issues.iter().any(|issue| {
        issue.severity == tokage_planning::ValidationSeverity::Error
            && issue.rule != "unconfirmed_acceptance_command"
    }) {
        return Err(serde_json::to_string(&result.issues).map_err(|e| e.to_string())?);
    }
    if result.plan.tasks.is_empty()
        || result
            .plan
            .tasks
            .iter()
            .any(|t| t.write_scope.is_empty() || t.acceptance.commands.is_empty())
    {
        return Err("CLI execution requires nonempty explicit write_scope and acceptance commands for every task".into());
    }
    run.plan = Some(result.plan);
    run.approval = None;
    run.status = Status::AwaitingApproval;
    run.error = None;
    Ok(())
}
pub async fn ensure_integration(store: &Store, run: &mut Run) -> Result<(), String> {
    let env = environment();
    if let Some(ref tree) = run.integration {
        if git::rev_parse(&env, Path::new(&tree.root), "HEAD").await?
            != run.integration_head.as_deref().unwrap_or(&run.target.head)
        {
            return Err("Integration head changed".into());
        }
    } else {
        let tree = git::create_worktree(
            &env,
            &run.repo,
            &store.root,
            &format!("cli-{}", run.id),
            Some(&run.target.head),
        )
        .await?;
        run.integration_head = Some(run.target.head.clone());
        run.integration = Some(tree);
        store.save(run)?;
    }
    Ok(())
}
pub fn block(store: &Store, run: &mut Run, error: &str) -> Result<(), String> {
    run.status = if error.starts_with("Run cancelled") {
        Status::Cancelled
    } else {
        Status::Blocked
    };
    run.error = Some(error.into());
    store.save(run)?;
    store.event(
        &run.id,
        &json!({"type":"blocked","status":run.status,"error":error}),
    )
}
