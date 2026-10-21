//! Stream actual native adapter events; the CLI never synthesizes model completion.
use crate::state::{nonce, Run, Store};
use serde_json::{json, Value};
use std::path::Path;
use tokage_core::{Bus, DomainEvent, EventKind, Role};
use tokage_native::{
    chat::{self, ChatRequest, TurnRegistry},
    env::ShellEnv,
    providers,
};

pub async fn run_turn(
    store: &Store,
    run: &Run,
    env: &ShellEnv,
    cwd: &Path,
    prompt: String,
    role: Role,
    phase: &str,
    scopes: &[String],
) -> Result<String, String> {
    let info = providers::detect_one(env, run.provider, run.provider_path.as_deref())
        .await
        .ok_or("Unknown provider")?;
    if !info.chat || info.path.is_none() {
        return Err("Provider is not installed or has no headless adapter".into());
    }
    let id = nonce();
    let dir = store.run_dir(&run.id)?;
    tokage_native::proc::set_run_dir(&dir.join("native"));
    tokage_native::recovery::claim_launch(
        &dir.join("turn-receipts"),
        &id,
        &json!({"turnId":id,"provider":run.provider,"model":run.model,"cwd":cwd,"role":role}),
    )
    .map_err(|e| e.to_string())?;
    let request: ChatRequest = serde_json::from_value(json!({
        "turnId":id,"cwd":cwd,"prompt":prompt,"mode":if role==Role::Planner{"plan"}else{"direct"},
        "provider":run.provider,"model":run.model,"resume":null,"trusted":[run.provider],
        "authMode":if role.allows_writes(){"full_auto"}else{"ask_sensitive"},
        "role":role,"writeScope":scopes,"timeoutTier":"AGENT","sessionId":run.id,"attemptId":id,
        "taskCode":phase,"phase":if role==Role::Planner{"PLANNING"}else{"EXECUTION"}
    }))
    .map_err(|e| e.to_string())?;
    let bus = Bus::new();
    let mut rx = bus.subscribe();
    let turns = TurnRegistry::new();
    store.event(&run.id,&json!({"type":"turnStarted","turnId":id,"phase":phase,"requestedProvider":run.provider,"requestedModel":run.model,"cwd":cwd,"role":role}))?;
    if !turns.spawn(
        bus,
        request,
        vec![info],
        env.clone(),
        Some(dir.join("mirror")),
    ) {
        return Err("Native turn launch refused".into());
    }
    let mut output = String::new();
    let mut done = false;
    let mut error = None;
    let result=async{
        loop{
            tokio::select!{
                event=rx.recv()=>match event{
                    Some(DomainEvent::ChatOutput{turn_id,event}) if turn_id==id=>{
                        let record=json!({"type":"chatEvent","turnId":id,"phase":phase,"event":event});
                        store.event(&run.id,&record)?;
                        // JSONL is streamed separately from the command's final stdout result.
                        eprintln!("{record}");
                        match event{
                            EventKind::Text{text}=>output.push_str(&text),
                            EventKind::Done{..}=>done=true,
                            EventKind::Error{message}=>error=Some(message),
                            EventKind::ApprovalRequest{reason,..}=>return Err(format!("Provider requires an unsupported interactive approval: {reason}")),
                            _=>{}
                        }
                    }
                    Some(DomainEvent::ChatEnded{turn_id,cancelled}) if turn_id==id=>{
                        store.event(&run.id,&json!({"type":"turnEnded","turnId":id,"phase":phase,"cancelled":cancelled,"doneObserved":done}))?;
                        if cancelled{return Err("Run cancelled".into());}
                        if let Some(e)=error{return Err(e);}
                        if !done{return Err("Native turn ended without structured Done evidence".into());}
                        return Ok(output)
                    }
                    None=>return Err("Native event stream closed before completion".into()),
                    _=>{}
                },
                _=tokio::signal::ctrl_c()=>return Err("Run cancelled by Ctrl-C".into()),
                _=tokio::time::sleep(std::time::Duration::from_millis(200))=>{
                    if store.cancelled(&run.id){return Err("Run cancelled".into());}
                }
            }
        }
    }.await;
    if result.is_err() {
        turns.cancel(&id).await;
    }
    result
}

pub fn parse_review(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let payload = trimmed
        .strip_prefix("```json")
        .and_then(|s| s.trim_end().strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    let value: Value =
        serde_json::from_str(payload).map_err(|e| format!("Invalid review verdict JSON: {e}"))?;
    if value.get("verdict").and_then(Value::as_str) != Some("PASS")
        || value
            .get("findings")
            .and_then(Value::as_array)
            .map(|v| !v.is_empty())
            .unwrap_or(true)
    {
        return Err(format!("Review did not pass: {value}"));
    }
    Ok(value)
}
