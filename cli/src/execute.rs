//! Headless orchestration using shared scheduling, worktree, scope and gate engines.
use crate::{
    environment,
    runs::{block, ensure_integration},
    state::{self, Run, Status, Store},
    turn,
};
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};
use tokage_core::{
    planning::{Plan, TaskItem},
    Role,
};
use tokage_native::{
    env::ShellEnv,
    git::{
        self,
        scope::{self, ScopeSnapshot},
    },
};
use tokage_orchestration::{self as orchestration, ConcurrencyLimits, DagScheduler, TaskState};

pub async fn execute(store: &Store, run: &mut Run) -> Result<(), String> {
    if run.status != Status::Approved {
        return Err("Run must be APPROVED; interrupted/blocked runs need explicit retry".into());
    }
    run.verify_approval()?;
    let result=async{
        ensure_integration(store,run).await?;
        run.status=Status::Executing;run.error=None;store.save(run)?;
        let env=environment();let plan=run.plan.clone().ok_or("Plan missing")?;
        let integration=run.integration.clone().ok_or("Integration checkout missing")?;
        let integration_path=PathBuf::from(&integration.root);
        if !git::is_working_tree_clean(&env,&integration_path).await?{return Err("Integration checkout is dirty".into());}
        loop{
            if store.cancelled(&run.id){return Err("Run cancelled".into());}
            let task=match pick(&plan,run)?{Some(t)=>t,None=>break};
            run.sequence=run.sequence.checked_add(1).ok_or("Attempt counter exhausted")?;store.save(run)?;
            let tree=git::create_worktree(&env,&run.repo,&store.root,&format!("cli-{}-{}",run.id,run.sequence),Some(run.integration_head.as_deref().ok_or("Integration identity missing")?)).await?;
            let cwd=Path::new(&tree.cwd);let before=scope::snapshot(&env,cwd).await?;
            let upstream=task.depends_on.iter().filter_map(|id|run.task_reports.get(id).map(|report|orchestration::UpstreamArtifact{task_id:id.clone(),title:id.clone(),report:report.clone(),changed_files:vec![]})).collect();
            let brief=orchestration::assemble_task_brief(&orchestration::TaskBriefInput{task:task.clone(),contracts:plan.contracts.clone(),upstream_artifacts:upstream,failure_evidence:None});
            let prompt=format!("{brief}\n\nDo not commit, change Git metadata or edit ignored/unrelated files. Tokage performs acceptance and commits. Original requirement:\n{}",run.requirement);
            store.event(&run.id,&json!({"type":"taskStarted","taskId":task.id,"sequence":run.sequence,"worktree":tree,"base":before.head}))?;
            let reply=turn::run_turn(store,run,&env,cwd,prompt,turn::TurnOptions {role:Role::Executor,phase:&format!("task-{}",run.sequence),scopes:&task.write_scope}).await?;
            if !reply.external_writes.is_empty(){return Err(format!("Executor reported writes outside its checkout: {:?}",reply.external_writes));}
            let native_changes=git::rollback::changes(&env,cwd,&reply.turn_id).await?;
            check_scope(&task,&native_changes)?;
            let after=scope::snapshot(&env,cwd).await?;
            if after.head!=before.head{return Err("Executor changed Git HEAD".into());}
            gates(store,run,&env,cwd,&task.acceptance.commands,&task.id).await?;
            let accepted=scope::snapshot(&env,cwd).await?;
            if accepted.head!=before.head{return Err("Gate changed Git HEAD".into());}
            let files=changed(&before,&accepted);check_scope(&task,&files)?;
            let report=orchestration::parse_or_generate_task_report(&task,&reply.text,&run.model,&reply.turn_id,run.sequence==1,"",files.clone(),"Independent CLI acceptance gates passed",None);
            store.event(&run.id,&json!({"type":"taskReport","report":report}))?;
            if report.is_locally_generated{return Err("Task report is missing required sections; no fabricated model report accepted".into());}
            if report.contract_deviations.is_some(){return Err("Task reported contract deviations; downstream execution blocked".into());}
            scope::commit_verified_with_options(&env,Path::new(&tree.root),&format!("feat: {} ({})",task.title,task.id),&accepted,run.co_author).await?;
            let head=git::rev_parse(&env,cwd,"HEAD").await?;
            match orchestration::merge_task_worktree_with_options(&env,&integration_path,&integration.branch,&tree.branch,&task,Some(&head),run.co_author).await?{
                orchestration::WorktreeMergeOutcome::Success{..}=>{},
                orchestration::WorktreeMergeOutcome::Conflict{diagnostic,..}=>return Err(format!("Integration conflict; preserved task worktree: {diagnostic}")),
            }
            run.integration_head=Some(git::rev_parse(&env,&integration_path,"HEAD").await?);
            run.completed_tasks.insert(task.id.clone(),head);run.task_reports.insert(task.id.clone(),report.to_markdown());store.save(run)?;
            store.event(&run.id,&json!({"type":"taskAccepted","taskId":task.id,"integrationHead":run.integration_head,"files":files}))?;
        }
        crate::delivery::accept(store, run, &env).await
    }.await;
    if let Err(e) = result {
        block(store, run, &e)?;
        return Err(e);
    }
    Ok(())
}

fn pick(plan: &Plan, run: &Run) -> Result<Option<TaskItem>, String> {
    if run.completed_tasks.len() == plan.tasks.len() {
        return Ok(None);
    }
    let states = plan
        .tasks
        .iter()
        .map(|t| {
            (
                t.id.clone(),
                if run.completed_tasks.contains_key(&t.id) {
                    TaskState::Done
                } else {
                    TaskState::Pending
                },
            )
        })
        .collect();
    let assignments = plan
        .tasks
        .iter()
        .map(|t| {
            (
                t.id.clone(),
                (run.provider.as_str().to_string(), run.model.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let scheduler = DagScheduler::from_states(
        &plan.tasks,
        &states,
        &assignments,
        ConcurrencyLimits {
            max_global_concurrent: 1,
            default_provider_concurrent: 1,
            ..Default::default()
        },
        &[],
    );
    let chosen = scheduler
        .pick()
        .dispatchable
        .into_iter()
        .next()
        .ok_or("No dispatchable task; DAG blocked")?;
    Ok(plan.tasks.iter().find(|t| t.id == chosen).cloned())
}
fn changed(before: &ScopeSnapshot, after: &ScopeSnapshot) -> Vec<String> {
    before
        .files
        .keys()
        .chain(after.files.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|p| before.files.get(*p) != after.files.get(*p))
        .cloned()
        .collect()
}
fn check_scope(task: &TaskItem, files: &[String]) -> Result<(), String> {
    let result = orchestration::validate_write_scope(&task.write_scope, files);
    if !result.passed {
        return Err(format!(
            "Out-of-scope changes: {:?}",
            result.out_of_scope_files
        ));
    }
    Ok(())
}

pub async fn gates(
    store: &Store,
    run: &Run,
    env: &ShellEnv,
    cwd: &Path,
    commands: &[String],
    phase: &str,
) -> Result<(), String> {
    let approval = run.verify_approval()?;
    for command in commands {
        crate::policy::guard_command(store, run, cwd, command)?;
        if store.cancelled(&run.id) {
            return Err("Run cancelled".into());
        }
        let id = state::nonce();
        let allow = &tokage_matching::pack::builtin_knowledge_pack().gate_allowlist;
        let request = orchestration::GateRequest {
            command,
            workdir: cwd,
            env,
            timeout_secs: None,
            confirmed: approval.confirmed_commands.contains(command),
            allowlist: allow,
            env_allow: &[],
            run_id: Some(&id),
        };
        let future = orchestration::execute_gate_command(request);
        tokio::pin!(future);
        let mut cancelled = false;
        let result = loop {
            tokio::select! {
                result=&mut future=>break result,
                _=tokio::signal::ctrl_c()=>{cancelled=true;orchestration::cancel_gate_command(&id);},
                _=tokio::time::sleep(std::time::Duration::from_millis(200))=>{
                    if store.cancelled(&run.id){cancelled=true;orchestration::cancel_gate_command(&id);}
                }
            }
        };
        store.event(
            &run.id,
            &json!({"type":"gateResult","phase":phase,"runId":id,"result":result}),
        )?;
        if cancelled {
            return Err("Run cancelled during gate".into());
        }
        if !result.passed {
            return Err(format!(
                "Gate failed: {command}: {}",
                result
                    .error_message
                    .as_deref()
                    .unwrap_or(&result.output_tail)
            ));
        }
    }
    Ok(())
}
