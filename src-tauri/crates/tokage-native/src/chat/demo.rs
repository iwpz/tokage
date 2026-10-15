//! Shared bilingual, deterministic demo fixture. Never starts a provider process.
use super::{
    debug::{DebugConfig, DebugOutput, MockScenario},
    ChatRequest, TurnMode,
};
use std::{path::Path, sync::OnceLock};
use tokage_core::Role;

pub fn fixture() -> &'static serde_json::Value {
    static FIXTURE: OnceLock<serde_json::Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../resources/demo/pocket-tasks.json"))
            .expect("bundled demo fixture")
    })
}

pub fn is_demo_workspace(cwd: &Path) -> bool {
    cwd.ancestors()
        .any(|p| p.join(".tokage-demo-workspace").is_file())
}

/// Persisted session identity and the tracked workspace marker both survive restarts.
/// Enforce before provider discovery, credentials, proxy setup or usage subscribers.
pub fn enforce_route(req: &mut ChatRequest) -> Result<(), String> {
    let session = req.session_id.as_deref().unwrap_or("");
    let session_demo = session.starts_with("tokage-demo-");
    let workspace_demo = is_demo_workspace(Path::new(&req.cwd));
    if !session_demo && !workspace_demo {
        return Ok(());
    }
    if !workspace_demo {
        return Err("Demo requests require an isolated demo workspace".into());
    }
    let mut config = req.debug.clone().unwrap_or_default();
    config.output = DebugOutput::Mock;
    config.demo = true;
    if session_demo {
        let parts: Vec<_> = session.split('-').collect();
        config.scenario = match parts.get(2).copied() {
            Some("success") => MockScenario::Success,
            Some("repair") => MockScenario::Repair,
            Some("error") => MockScenario::Error,
            Some("slow") => MockScenario::Slow,
            _ => return Err("Invalid demo scenario".into()),
        };
        config.demo_language = match parts.get(3).copied() {
            Some("en") => "en".into(),
            Some("zh") => "zh".into(),
            _ => return Err("Invalid demo language".into()),
        };
    }
    req.debug = Some(config);
    Ok(())
}

pub fn language(config: &DebugConfig) -> &str {
    if config.demo_language == "zh" {
        "zh"
    } else {
        "en"
    }
}
pub fn text<'a>(config: &DebugConfig, key: &'a str) -> &'a str {
    fixture()["locales"][language(config)][key]
        .as_str()
        .unwrap_or(key)
}
pub fn reply(req: &ChatRequest, config: &DebugConfig) -> String {
    let script = &fixture()["locales"][language(config)];
    if req.mode == TurnMode::Plan || req.effective_role() == Role::Planner {
        return script["plan"]["rawMarkdown"].as_str().unwrap().into();
    }
    let task = script["plan"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"].as_str() == req.task_code.as_deref());
    let title = task
        .and_then(|t| t["title"].as_str())
        .unwrap_or("Pocket Tasks");
    format!("{}\n\n### What was completed\n{title}\n{}\n### Deviations from contracts\n{}\n### Hints for downstream\n{}\n### Self-check against acceptance criteria\n- {}\n### Defects found\n{}\n", text(config, "working"), text(config, "completed"), text(config, "none"), text(config, "downstream"), text(config, "acceptance"), text(config, "none"))
}
pub fn file(req: &ChatRequest, config: &DebugConfig, path: &str) -> Option<String> {
    if path == "README.md" {
        return Some(text(config, "readme").into());
    }
    let content = fixture()["files"][path].as_str()?;
    if path == "src/task-service.js"
        && config.scenario == MockScenario::Repair
        && req.effective_phase() != "REPAIR"
    {
        return Some(content.replace("export function remainingCount(tasks) { return tasks.filter(t => !t.completed).length; }", "export function remainingCount(tasks) { return tasks.length; }"));
    }
    Some(content.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::ProviderId;

    #[test]
    fn demo_identity_cannot_escape_to_a_real_provider_after_settings_change() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".tokage-demo-workspace"), "demo").unwrap();
        let mut req = super::super::testing::request(ProviderId::ClaudeCode);
        req.cwd = root.path().to_string_lossy().into();
        req.session_id = Some("tokage-demo-repair-zh-run".into());
        req.debug = Some(DebugConfig {
            output: DebugOutput::Luna,
            ..Default::default()
        });
        enforce_route(&mut req).unwrap();
        let config = req.debug.as_ref().unwrap();
        assert_eq!(config.output, DebugOutput::Mock);
        assert_eq!(config.scenario, MockScenario::Repair);
        assert_eq!(language(config), "zh");
        req.cwd = root.path().join("missing").to_string_lossy().into();
        // A descendant retains the marker, just as the project's worktree cwd does.
        enforce_route(&mut req).unwrap();
        let other = tempfile::tempdir().unwrap();
        req.cwd = other.path().to_string_lossy().into();
        assert!(enforce_route(&mut req).is_err());
    }

    #[test]
    fn ordinary_requests_keep_their_execution_configuration() {
        let mut req = super::super::testing::request(ProviderId::ClaudeCode);
        req.session_id = Some("ordinary-session".into());
        req.debug = None;
        enforce_route(&mut req).unwrap();
        assert!(req.debug.is_none());
    }
}
