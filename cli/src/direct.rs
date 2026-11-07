//! Public direct-work transport. The same native execution service owns desktop
//! and CLI journals; request files cannot grant workspace trust or Full-auto.
use crate::{environment, options, required, state::Store};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokage_native::{
    chat::{ChatRequest, TurnMode},
    direct::{self, Execution},
};

pub async fn dispatch(
    action: &str,
    raw: &[String],
    directory: Option<PathBuf>,
) -> Result<Value, String> {
    let store = Store::new(directory)?;
    let root = store.root.join("direct-v1");
    match action {
        "history" => {
            let o = options(raw, &["--session"], &[])?;
            let session = required(&o, "--session")?;
            serde_json::to_value(direct::list(&root, Some(&session))?).map_err(|e| e.to_string())
        }
        "events" | "reconcile" => {
            let o = options(raw, &["--turn"], &[])?;
            let id = required(&o, "--turn")?;
            if action == "events" {
                serde_json::to_value(direct::events(&root, &id)?)
            } else {
                serde_json::to_value(direct::reconcile(&root, &id)?)
            }
            .map_err(|e| e.to_string())
        }
        "send" => {
            let o = options(
                raw,
                &["--request", "--provider-path"],
                &["--trust-workspace", "--full-auto"],
            )?;
            if !o.contains_key("--trust-workspace") {
                return Err("Direct work requires explicit --trust-workspace".into());
            }
            let text =
                std::fs::read_to_string(required(&o, "--request")?).map_err(|e| e.to_string())?;
            let mut req: ChatRequest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if req.mode != TurnMode::Direct
                || req.plan_id.is_some()
                || req.task_id.is_some()
                || req.task_code.is_some()
                || req.role.is_some_and(|r| r != tokage_core::Role::Executor)
                || req
                    .debug
                    .as_ref()
                    .is_some_and(|d| d.output != tokage_native::chat::debug::DebugOutput::Off)
            {
                return Err(
                    "Use a direct executor request without plan ownership or debug routing".into(),
                );
            }
            if req.auth_mode == Some(tokage_core::AuthMode::FullAuto)
                && !o.contains_key("--full-auto")
            {
                return Err("Full-auto additionally requires explicit --full-auto; a request file cannot grant it".into());
            }
            let cwd = std::fs::canonicalize(&req.cwd).map_err(|e| e.to_string())?;
            // Match CLI run isolation: never write execution records into the project.
            let mut ancestor = store.root.as_path();
            while !ancestor.exists() {
                ancestor = ancestor.parent().ok_or("Invalid data directory")?;
            }
            if ancestor
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&cwd)
            {
                return Err("CLI data directory must be outside workspace".into());
            }
            req.cwd = cwd.to_string_lossy().into_owned();
            req.trusted = vec![req.provider];
            let processes = root
                .join("processes")
                .join(tokage_runtime::digest(&req.turn_id).map_err(|e| e.to_string())?);
            req.process_registry_dir = Some(processes.clone());
            let custom = if o.contains_key("--provider-path") {
                Some(required(&o, "--provider-path")?)
            } else {
                None
            };
            let env = environment();
            let provider =
                tokage_native::providers::detect_one(&env, req.provider, custom.as_deref())
                    .await
                    .ok_or("Provider is not registered")?;
            let id = req.turn_id.clone();
            let execution = Execution::prepare(&root, &req)?;
            let bus = tokage_core::Bus::new();
            let (stop, cancel) = tokio::sync::watch::channel(false);
            let run = execution.run(&bus, req, vec![provider], env, None, Some(cancel));
            tokio::pin!(run);
            tokio::select! {
                _=&mut run=>{},
                _=tokio::signal::ctrl_c()=>{let _=stop.send(true);run.await;},
            }
            Ok(
                json!({"turn":direct::list(&root,None)?.into_iter().find(|r|r.turn_id==id),"events":direct::events(&root,&id)?}),
            )
        }
        _ => Err("Unknown direct command; use send, history, events or reconcile".into()),
    }
}
