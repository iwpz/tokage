//! Headless application service. Durable intent and claim precede planner side effects.
use crate::{repository::SqliteRepository, *};
use std::{
    future::Future,
    path::{Path, PathBuf},
};
use tokage_core::{Bus, DomainEvent, EventKind as NativeEvent, Role, UsageSource};
use tokage_native::{
    chat::{
        adapter::{adapter, Caps},
        ChatRequest, TurnRegistry,
    },
    env::ShellEnv,
    git, providers,
};

/// Adapter boundary for a real read-only planner. Offline implementations belong in tests.
pub trait Planner {
    fn generate(
        &self,
        snapshot: &Snapshot,
        attempt_id: &str,
    ) -> impl Future<Output = Result<String>> + Send;
}
/// No terminal, Tauri, UI subscription, environment mutation or hidden fixture routing.
pub struct NativePlanner {
    env: ShellEnv,
    directory: PathBuf,
}
impl NativePlanner {
    pub fn new(env: ShellEnv, directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory).map_err(|e| Error::Repository(e.to_string()))?;
        Ok(Self {
            env,
            directory: directory
                .canonicalize()
                .map_err(|e| Error::Repository(e.to_string()))?,
        })
    }
}
impl Planner for NativePlanner {
    async fn generate(&self, snapshot: &Snapshot, attempt_id: &str) -> Result<String> {
        let invalid = |e: String| Error::Invalid(e);
        let selected = snapshot
            .config
            .models
            .get("planner")
            .or_else(|| snapshot.config.models.get("executor"))
            .ok_or_else(|| invalid("planner assignment required".into()))?;
        let provider = serde_json::from_value(serde_json::json!(selected.provider))
            .map_err(|e| invalid(e.to_string()))?;
        let caps = adapter(provider).ok_or(Error::Unsupported)?.meta().caps;
        if !caps.has(Caps::READ_ONLY_MODE) || !caps.has(Caps::STRUCTURED_OUTPUT) {
            return Err(Error::Unsupported);
        }
        let info = providers::detect_one(&self.env, provider, selected.provider_path.as_deref())
            .await
            .ok_or(Error::Unsupported)?;
        if !info.chat || info.path.is_none() {
            return Err(Error::Unsupported);
        }
        let cwd = Path::new(&snapshot.config.workspace)
            .canonicalize()
            .map_err(|e| invalid(e.to_string()))?;
        if git::rev_parse(&self.env, &cwd, "HEAD")
            .await
            .map_err(invalid)?
            != snapshot.config.target_identity
        {
            return Err(invalid("planning target changed".into()));
        }
        let before = git::scope::snapshot(&self.env, &cwd)
            .await
            .map_err(invalid)?;
        let digest = tokage_planning::RepoDigestBuilder::new(&cwd).build().await;
        let pack = planning::effective_pack(&snapshot.config)?;
        let prompt = tokage_planning::render_planning_prompt(tokage_planning::PlanningPromptArgs {
            user_request: &snapshot.requirement,
            repo_digest: &digest.markdown,
            domains: Some(&pack.domains),
            output_max_tokens: None,
            custom_format_spec: None,
        });
        let dir = self
            .directory
            .join(crate::digest(&snapshot.run_id)?)
            .join(attempt_id);
        std::fs::create_dir_all(&dir).map_err(|e| invalid(e.to_string()))?;
        tokage_native::recovery::claim_launch(
            &dir.join("receipts"),
            attempt_id,
            &serde_json::json!({"turnId":attempt_id,"cwd":cwd}),
        )
        .map_err(|e| invalid(e.to_string()))?;
        let mut request: ChatRequest = serde_json::from_value(serde_json::json!({
            "turnId":attempt_id,"cwd":cwd,"prompt":prompt,"mode":"plan","provider":provider,
            "model":selected.requested_model,"resume":null,"trusted":[provider],"role":Role::Planner,
            "authMode":"ask_sensitive","sessionId":snapshot.run_id,"attemptId":attempt_id,"phase":"PLANNING",
            "timeoutTier":"AGENT","idleTimeoutSecs":snapshot.config.timeout_seconds,"hardTimeoutSecs":snapshot.config.timeout_seconds
        })).map_err(|e|invalid(e.to_string()))?;
        request.process_registry_dir = Some(dir.join("processes"));
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let turns = TurnRegistry::new();
        if !turns.spawn(
            bus,
            request,
            vec![info],
            self.env.clone(),
            Some(dir.join("mirror")),
        ) {
            return Err(invalid("launch refused".into()));
        }
        let redactor = tokage_native::chat::redact::Redactor::new(&self.env);
        let mut output = String::new();
        let mut done = false;
        let mut failure = None;
        let result = async {
            loop {
                match rx.recv().await {
                    Some(DomainEvent::ChatOutput { turn_id, event }) if turn_id == attempt_id => {
                        match redactor.redact_event(event) {
                            NativeEvent::Text { text } => output.push_str(&text),
                            NativeEvent::Done { source, .. } => {
                                done |= source == UsageSource::Structured
                            }
                            NativeEvent::Error { message } => failure = Some(message),
                            NativeEvent::ApprovalRequest { .. } => {
                                return Err(invalid("planner requires interactive approval".into()))
                            }
                            NativeEvent::FileChange { .. } => {
                                return Err(invalid("read-only planner reported a write".into()))
                            }
                            _ => {}
                        }
                    }
                    Some(DomainEvent::ChatEnded { turn_id, cancelled })
                        if turn_id == attempt_id =>
                    {
                        if cancelled || !done || failure.is_some() {
                            return Err(invalid(
                                failure
                                    .unwrap_or_else(|| "planner ended without completion".into()),
                            ));
                        }
                        return Ok(output);
                    }
                    None => return Err(invalid("planner event stream disconnected".into())),
                    _ => {}
                }
            }
        }
        .await;
        turns.cancel(attempt_id).await;
        if !tokage_native::recovery::runtime_idle(Some(&dir.join("processes")))
            .map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid("planner process cleanup not confirmed".into()));
        }
        if git::scope::snapshot(&self.env, &cwd)
            .await
            .map_err(invalid)?
            != before
        {
            return Err(invalid("read-only planner changed workspace".into()));
        }
        result
    }
}
/// Execute a previously committed planning intent once. A lost/unknown effect result is
/// deliberately blocked on retry, never sent to the model a second time.
pub async fn drive_planning(
    repo: &mut SqliteRepository,
    run_id: &str,
    planner: &impl Planner,
) -> Result<()> {
    let snapshot = repo.snapshot(run_id)?.ok_or(Error::Transition)?;
    if snapshot.state != RunState::Planning {
        return Err(Error::Transition);
    }
    let attempt = snapshot.planning_attempt.clone().ok_or(Error::Transition)?;
    if attempt.claimed {
        return Err(Error::Invalid(
            "planning launch already claimed; recovery required".into(),
        ));
    }
    repo.advance(snapshot.revision, |s| {
        s.planning_attempt
            .as_mut()
            .ok_or(Error::Transition)?
            .claimed = true;
        Ok(EventKind::PlanningClaimed)
    })
    .await?;
    let result = planner
        .generate(&snapshot, &attempt.id)
        .await
        .and_then(|text| planning::normalize(&text, &snapshot.config));
    let current = repo.snapshot(run_id)?.ok_or(Error::Transition)?;
    let failure = result.as_ref().err().cloned();
    repo.advance(current.revision, move |s| {
        if s.state != RunState::Planning
            || s.planning_attempt.as_ref().map(|a| a.id.as_str()) != Some(attempt.id.as_str())
        {
            return Err(Error::Transition);
        }
        match result {
            Ok(plan) => {
                s.plan = Some(plan);
                s.state = RunState::AwaitingApproval;
                Ok(EventKind::PlanningCompleted)
            }
            Err(error) => {
                s.state = RunState::Blocked;
                s.planning_attempt.as_mut().unwrap().error = Some(error.to_string());
                Ok(EventKind::PlanningFailed)
            }
        }
    })
    .await?;
    if let Some(e) = failure {
        return Err(e);
    }
    Ok(())
}
