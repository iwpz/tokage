//! CLI authorization uses the same built-in command policy as the desktop service.
use crate::state::{Run, Store};
use std::path::Path;
use tokage_core::command_policy::{Context, Level, PackPolicy, Policy};

pub fn guard_command(store: &Store, run: &Run, cwd: &Path, command: &str) -> Result<(), String> {
    let pack: PackPolicy = serde_json::from_value(
        tokage_matching::pack::builtin_knowledge_pack()
            .command_policy
            .clone()
            .ok_or("Built-in command policy unavailable")?,
    )
    .map_err(|e| e.to_string())?;
    let mut context = Context::new(cwd.to_string_lossy().to_string());
    context.workspace_roots = vec![cwd.to_string_lossy().to_string()];
    context.home = std::env::var("HOME").ok();
    // Control state is protected; worktrees are sibling workspaces, not app metadata.
    context.app_data = Some(
        store
            .root
            .join("runs")
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .to_string(),
    );
    context.provider = Some(run.provider.as_str().to_string());
    context.current_branch = Some(
        run.integration
            .as_ref()
            .map(|w| w.branch.clone())
            .unwrap_or_else(|| run.target.reference.clone()),
    );
    let verdict = Policy::new(&pack, &[]).classify(command, &context);
    if verdict.level == Level::Forbidden {
        return Err(format!("Command is forbidden by shared policy: {command}"));
    }
    Ok(())
}
