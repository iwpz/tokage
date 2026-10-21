//! Headless application service. Durable intent and claim precede planner side effects.
use crate::*;
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
pub struct TurnControl {
    pub events: tokio::sync::mpsc::Sender<NativeEvent>,
    pub cancel: tokio::sync::watch::Receiver<bool>,
}
pub trait Planner: Send + Sync {
    fn generate_observed(
        &self,
        snapshot: &Snapshot,
        attempt_id: &str,
        control: TurnControl,
    ) -> impl Future<Output = Result<String>> + Send;
}
/// No terminal, Tauri, UI subscription, environment mutation or hidden fixture routing.
#[derive(Clone)]
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
    async fn generate_observed(
        &self,
        snapshot: &Snapshot,
        attempt_id: &str,
        control: TurnControl,
    ) -> Result<String> {
        self.run(snapshot, attempt_id, Some(control)).await
    }
}
impl NativePlanner {
    async fn run(
        &self,
        snapshot: &Snapshot,
        attempt_id: &str,
        mut control: Option<TurnControl>,
    ) -> Result<String> {
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
        if self.directory.starts_with(&cwd) {
            return Err(invalid("runtime data must be outside the workspace".into()));
        }
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
        if control.as_ref().is_some_and(|c| *c.cancel.borrow()) {
            return Err(Error::Cancelled);
        }
        let continuation = snapshot
            .planning_attempt
            .as_ref()
            .and_then(|a| a.continuation_of.as_ref())
            .and_then(|id| snapshot.turns.get(id));
        let allowed: Vec<String> = continuation
            .and_then(|t| t.approval.as_ref())
            .filter(|a| {
                matches!(
                    a.decision,
                    Some(ApprovalDecision::AllowOnce | ApprovalDecision::AllowSession)
                )
            })
            .map(|a| vec![a.command.clone()])
            .unwrap_or_default();
        let resume = continuation.and_then(|t| t.thread_id.clone());
        let mut request: ChatRequest = serde_json::from_value(serde_json::json!({
            "turnId":attempt_id,"cwd":cwd,"prompt":prompt,"mode":"plan","provider":provider,
            "model":selected.requested_model,"resume":resume,"sessionAllowed":allowed,"trusted":[provider],"role":Role::Planner,
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
                if rx.len()>8192 {return Err(invalid("native event backlog exceeded; execution stopped without accepting completion".into()));}
                let incoming=if let Some(c)=&mut control {
                    tokio::select! {e=rx.recv()=>e,_=c.cancel.changed()=>return Err(Error::Cancelled)}
                } else {rx.recv().await};
                match incoming {
                    Some(DomainEvent::ChatOutput { turn_id, event }) if turn_id == attempt_id => {
                        let event=redactor.redact_event(event);
                        if let Some(c)=&control {if c.events.send(event.clone()).await.is_err(){return Err(Error::Cancelled);}}
                        match event {
                            NativeEvent::Text { text } => output.push_str(&text),
                            NativeEvent::Done { source, .. } => {
                                done |= source == UsageSource::Structured
                            }
                            NativeEvent::Error { message } => failure = Some(message),
                            NativeEvent::ApprovalRequest { .. } => {
                                return Err(Error::AwaitingApproval)
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
            .map_err(|_| Error::CleanupUnknown)?
        {
            return Err(Error::CleanupUnknown);
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
