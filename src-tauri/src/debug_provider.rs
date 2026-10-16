//! App integration for the Debug Provider: isolated fixture repositories and catalogs.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokage_native::chat::debug::{DebugConfig, DebugOutput, MockScenario};

#[derive(Default)]
pub struct DebugState {
    pub workspace_lock: tokio::sync::Mutex<()>,
    turns: Mutex<HashMap<String, DebugConfig>>,
    gates: Mutex<HashMap<String, usize>>,
}

impl DebugState {
    pub fn remember(&self, cwd: &str, config: DebugConfig) {
        self.turns.lock().unwrap().insert(cwd.into(), config);
    }
    pub fn remember_request(&self, req: &tokage_native::chat::ChatRequest, config: DebugConfig) {
        self.remember(&req.cwd, config.clone());
        let demo_repair = config.demo && req.effective_phase() == "REPAIR";
        if let (Some(session), Some(task)) = (&req.session_id, &req.task_code) {
            let key = format!("{}:gate:{session}:{task}", req.cwd);
            self.turns.lock().unwrap().insert(key.clone(), config);
            if req.resume.is_none() && req.effective_phase() != "REPAIR" {
                self.gates.lock().unwrap().remove(&key);
            } else if demo_repair {
                self.gates
                    .lock()
                    .unwrap()
                    .entry(key)
                    .and_modify(|n| *n = (*n).max(1))
                    .or_insert(1);
            }
        }
    }

    pub fn gate(
        &self,
        command: String,
        cwd: &str,
        run_id: Option<&str>,
        config: Option<DebugConfig>,
    ) -> Option<tokage_orchestration::GateCommandResult> {
        if !tokage_native::chat::debug::is_mock_workspace(Path::new(cwd)) {
            return None;
        }
        let key = format!("{cwd}:{}", run_id.unwrap_or("delivery"));
        let remembered = {
            let turns = self.turns.lock().unwrap();
            turns.get(&key).or_else(|| turns.get(cwd)).cloned()
        };
        let config = remembered
            .or(config)
            .filter(|c| c.output == DebugOutput::Mock);
        let mut gates = self.gates.lock().unwrap();
        let count = gates.entry(key).or_default();
        let passed = if config.as_ref().is_some_and(|c| c.demo) {
            !config
                .as_ref()
                .is_some_and(|c| c.scenario == MockScenario::Repair)
                || !run_id.is_some_and(|id| id.ends_with(":T2"))
                || *count > 0
        } else {
            match config.as_ref().map(|c| c.scenario) {
                Some(MockScenario::Repair) => {
                    run_id.is_some_and(|id| id.starts_with("gate:delivery:")) || *count > 0
                }
                Some(MockScenario::Error) | None => false,
                _ => true,
            }
        };
        *count += 1;
        Some(tokage_orchestration::GateCommandResult {
            command,
            passed,
            exit_code: Some(if passed { 0 } else { 1 }),
            duration_ms: 0,
            output_tail: if let Some(c) = config.as_ref().filter(|c| c.demo) {
                tokage_native::chat::demo::text(c, if passed { "gatePass" } else { "gateFail" })
                    .into()
            } else if passed {
                "[Debug Provider] Simulated acceptance passed; no command was executed.".into()
            } else {
                "[Debug Provider] Simulated acceptance failed. Repair the task and retry.".into()
            },
            error_message: None,
        })
    }
}

/// A fresh, deterministic repo. Real Git/worktree/file code runs only in this fixture.
pub fn prepare_workspace(base: &Path, original: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let original_path = Path::new(original);
    if tokage_native::chat::debug::is_mock_workspace(original_path) {
        return Ok(original.into());
    }
    let hash = format!("{:x}", Sha256::digest(original.as_bytes()));
    let root = if let Some(run) = original.strip_prefix("tokage-demo:") {
        if run.is_empty() || !run.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err("Invalid demo run identity".into());
        }
        base.join("demo").join(run).join("demo_project")
    } else {
        base.join(&hash[..16])
    };
    if root.join(".git/refs/heads/main").is_file() {
        return Ok(root.display().to_string());
    }
    std::fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("tests")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("empty-hooks")).map_err(|e| e.to_string())?;
    std::fs::write(
        root.join(".tokage-debug-workspace"),
        "Tokage Debug Provider fixture\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(root.join("package.json"), r#"{"name":"tokage-debug-fixture","private":true,"scripts":{"test":"node -e \"process.exit(0)\"","build":"node -e \"process.exit(0)\""}}"#).map_err(|e| e.to_string())?;
    std::fs::write(
        root.join("README.md"),
        "# Tokage Debug Provider\n\nIsolated fixture for simulated runs.\n",
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(root.join(".gitignore"), ".tokage/\nempty-hooks/\n")
        .map_err(|e| e.to_string())?;
    if original.starts_with("tokage-demo:") {
        std::fs::write(root.join(".tokage-demo-workspace"), "Pocket Tasks demo\n")
            .map_err(|e| e.to_string())?;
        for (path, content) in tokage_native::chat::demo::fixture()["initialFiles"]
            .as_object()
            .unwrap()
        {
            let target = root.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(target, content.as_str().unwrap()).map_err(|e| e.to_string())?;
        }
    }
    // Keep all later real Git operations deterministic too (including merges/commits).
    // Configuration belongs solely to the fixture repository.
    // No hooks, signing, remote or account required. Never uses the original repository.
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "user.name", "Tokage Debug"],
        vec!["config", "user.email", "debug@tokage.invalid"],
        vec!["config", "core.hooksPath", "empty-hooks"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Tokage Debug",
            "-c",
            "user.email=debug@tokage.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Initialize debug fixture",
        ],
    ] {
        let out = std::process::Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                root.join("empty-hooks").display()
            ))
            .args(args)
            .current_dir(&root)
            .output()
            .map_err(|e| format!("Debug Provider fixture: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "Debug Provider fixture: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    Ok(root.display().to_string())
}

/// Build catalog identities from the same effective knowledge pack used by matching.
/// No installation/login/CLI probing is needed to select a simulated model.
pub fn catalog(pack: &tokage_matching::EffectiveKnowledgePack) -> Vec<serde_json::Value> {
    let mut providers: std::collections::BTreeMap<
        String,
        Vec<tokage_native::providers::ModelInfo>,
    > = std::collections::BTreeMap::new();
    for c in &pack.candidates {
        if serde_json::from_value::<tokage_core::ProviderId>(serde_json::json!(c.provider)).is_err()
        {
            continue;
        }
        providers.entry(c.provider.clone()).or_default().push(
            tokage_native::providers::ModelInfo {
                id: c.cli_model_arg.clone(),
                name: c.display_name.clone(),
            },
        );
    }
    providers.into_iter().map(|(id, mut models)| {
        models.sort_by(|a, b| a.id.cmp(&b.id)); models.dedup_by(|a, b| a.id == b.id);
        let groups = tokage_native::models::group(&models);
        serde_json::json!({
            "id": id, "name": id.replace('_', " "), "executable": "debug", "featured": true,
            "status": "READY", "version": null, "path": null, "authMethod": "debug",
            "models": models, "modelGroups": groups, "modelsSource": "BUILTIN", "account": null,
            "guidance": [], "installMethods": [], "trustArgs": [], "chat": true,
            "adapter": { "caps": ["STRUCTURED_OUTPUT", "RESUME", "MODEL_SELECT", "READ_ONLY_MODE"], "versions": "debug", "tested": "debug" },
            "directLimit": null, "debugVirtual": true,
        })
    }).collect()
}

pub fn workspace_base(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("debug-workspaces"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokage_core::{Bus, DomainEvent, EventKind, ProviderId, Role};
    use tokage_native::{
        chat::{self, debug::MOCK_PLAN, TimeoutTier, TurnMode},
        env::ShellEnv,
        git,
    };

    fn effective_pack() -> tokage_matching::EffectiveKnowledgePack {
        tokage_matching::merge_layers(&tokage_matching::LayeredPacks {
            builtin: tokage_matching::builtin_knowledge_pack().clone(),
            official: None,
            customs: vec![],
            overrides: vec![],
            learned_usage_profiles: None,
        })
        .unwrap()
    }

    #[test]
    fn fixture_preserves_original_and_is_reused() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("untouched.txt"), "original").unwrap();
        let root =
            prepare_workspace(&dir.path().join("debug"), original.to_str().unwrap()).unwrap();
        assert!(!Path::new(&root).starts_with(&original));
        assert_eq!(
            std::fs::read_to_string(original.join("untouched.txt")).unwrap(),
            "original"
        );
        assert!(!original.join(".git").exists());
        assert!(tokage_native::chat::debug::is_mock_workspace(Path::new(
            &root
        )));
        let package: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(Path::new(&root).join("package.json")).unwrap(),
        )
        .unwrap();
        assert!(package["scripts"]["test"].is_string());
        assert_eq!(
            prepare_workspace(&dir.path().join("debug"), original.to_str().unwrap()).unwrap(),
            root
        );
        assert_eq!(
            prepare_workspace(&dir.path().join("debug"), &root).unwrap(),
            root
        );
    }

    #[test]
    fn debug_catalog_and_plan_use_production_matching_and_validation() {
        let pack = effective_pack();
        let catalog = catalog(&pack);
        let count: usize = catalog
            .iter()
            .map(|p| p["models"].as_array().unwrap().len())
            .sum();
        let offerings: std::collections::HashSet<_> = pack
            .candidates
            .iter()
            .map(|c| (&c.provider, &c.cli_model_arg))
            .collect();
        assert_eq!(count, offerings.len());
        let plan = tokage_planning::parse_plan(MOCK_PLAN, &Default::default()).unwrap();
        let validation = tokage_planning::validate_plan(
            plan,
            &tokage_planning::PlanValidationOptions {
                allowed_commands: vec!["npm test".into()],
                ..Default::default()
            },
        );
        assert!(validation.is_valid, "{:?}", validation.issues);
        assert_eq!(validation.plan.tasks[1].depends_on, vec!["T1"]);
        let allocation = tokage_matching::allocate_plan_models(
            &validation.plan,
            &pack,
            &tokage_matching::LearnedIndex::empty(),
            None,
            None,
        );
        assert_eq!(allocation.assignments.len(), 2);
    }

    #[test]
    fn concurrent_tasks_keep_their_scenario_and_repair_state_independent() {
        let dir = tempfile::tempdir().unwrap();
        let root = prepare_workspace(&dir.path().join("debug"), "project").unwrap();
        let state = DebugState::default();
        let mut a = chat::testing::request(ProviderId::ClaudeCode);
        a.cwd = root.clone();
        a.session_id = Some("s".into());
        a.task_code = Some("T1".into());
        let repair = DebugConfig {
            output: DebugOutput::Mock,
            scenario: MockScenario::Repair,
            ..Default::default()
        };
        state.remember_request(&a, repair.clone());
        assert!(
            !state
                .gate("npm test".into(), &root, Some("gate:s:T1"), None)
                .unwrap()
                .passed
        );
        let mut b = a.clone();
        b.task_code = Some("T2".into());
        state.remember_request(
            &b,
            DebugConfig {
                output: DebugOutput::Mock,
                scenario: MockScenario::Success,
                ..Default::default()
            },
        );
        assert!(
            state
                .gate("npm test".into(), &root, Some("gate:s:T2"), None)
                .unwrap()
                .passed
        );
        a.phase = Some("REPAIR".into());
        state.remember_request(&a, repair.clone());
        assert!(
            state
                .gate("npm test".into(), &root, Some("gate:s:T1"), None)
                .unwrap()
                .passed
        );
        a.phase = Some("EXECUTION".into());
        state.remember_request(&a, repair);
        assert!(
            !state
                .gate("npm test".into(), &root, Some("gate:s:T1"), None)
                .unwrap()
                .passed
        );
        assert!(
            state
                .gate("npm test".into(), &root, Some("gate:s:T2"), None)
                .unwrap()
                .passed
        );
    }

    #[tokio::test]
    async fn mock_pipeline_uses_real_worktrees_files_reports_and_repair_feedback() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        std::fs::create_dir(&original).unwrap();
        let root =
            prepare_workspace(&dir.path().join("debug"), original.to_str().unwrap()).unwrap();
        let mut env = ShellEnv {
            vars: std::env::vars().collect(),
        };
        // An inherited repository override must never redirect fixture Git operations.
        env.vars.insert(
            "GIT_DIR".into(),
            original.join("must-not-use.git").display().to_string(),
        );
        env.vars
            .insert("GIT_WORK_TREE".into(), original.display().to_string());
        let integration =
            git::create_worktree(&env, Path::new(&root), dir.path(), "integration", None)
                .await
                .unwrap();
        let plan = tokage_planning::parse_plan(MOCK_PLAN, &Default::default()).unwrap();
        let state = DebugState::default();
        let config = DebugConfig {
            output: DebugOutput::Mock,
            scenario: MockScenario::Repair,
            ..Default::default()
        };
        for (index, task) in plan.tasks.iter().enumerate() {
            let worktree = git::create_worktree(
                &env,
                Path::new(&root),
                dir.path(),
                &format!("task-{index}"),
                Some(&integration.branch),
            )
            .await
            .unwrap();
            if index == 1 {
                assert!(
                    Path::new(&worktree.cwd)
                        .join("src/debug-result.ts")
                        .is_file(),
                    "downstream must see merged upstream"
                );
            }
            state.remember(&worktree.cwd, config.clone());
            let mut req = chat::testing::request(ProviderId::ClaudeCode);
            req.cwd = worktree.cwd.clone();
            req.turn_id = format!("mock-{index}");
            req.mode = TurnMode::Direct;
            req.role = Some(if index == 0 {
                Role::Executor
            } else {
                Role::Tester
            });
            req.timeout_tier = Some(TimeoutTier::Agent);
            req.task_id = Some(index as i64 + 1);
            req.write_scope = task.write_scope.clone();
            req.debug = Some(config.clone());
            req.prompt =
                tokage_orchestration::assemble_task_brief(&tokage_orchestration::TaskBriefInput {
                    task: task.clone(),
                    contracts: plan.contracts.clone(),
                    upstream_artifacts: vec![],
                    failure_evidence: None,
                });
            let bus = Bus::new();
            let mut events = bus.subscribe();
            let registry = chat::TurnRegistry::new();
            registry.spawn_mock(bus.clone(), req.clone());
            let mut raw = String::new();
            let mut files = vec![];
            let mut done = 0;
            loop {
                let ev = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap();
                match ev {
                    DomainEvent::ChatOutput {
                        event: EventKind::Text { text },
                        ..
                    } => raw.push_str(&text),
                    DomainEvent::ChatOutput {
                        event: EventKind::FileChange { path, .. },
                        ..
                    } => files.push(path),
                    DomainEvent::ChatOutput {
                        event: EventKind::Done { api_calls, .. },
                        ..
                    } => {
                        assert_eq!(api_calls, Some(0));
                        done += 1;
                    }
                    DomainEvent::ChatOutput {
                        event: EventKind::Error { message },
                        ..
                    } => panic!("{message}"),
                    DomainEvent::ChatEnded { cancelled, .. } => {
                        assert!(!cancelled);
                        break;
                    }
                    _ => {}
                }
            }
            assert_eq!(done, 1);
            assert_eq!(files, task.write_scope);
            let report = tokage_orchestration::parse_or_generate_task_report(
                task,
                &raw,
                "mock",
                &req.turn_id,
                true,
                "1 file",
                files,
                "simulated",
                Some(0.0),
            );
            assert!(
                !report.is_locally_generated,
                "mock must exercise the real report parser"
            );
            let run_id = format!("gate:s:{}", task.id);
            let command = format!("touch {}", original.join("must-not-exist").display());
            assert!(
                !state
                    .gate(command.clone(), &worktree.cwd, Some(&run_id), None)
                    .unwrap()
                    .passed
            );
            assert!(
                state
                    .gate(command, &worktree.cwd, Some(&run_id), None)
                    .unwrap()
                    .passed
            );
            assert!(!original.join("must-not-exist").exists());
            git::commit_worktree(&env, Path::new(&worktree.root), "Mock task complete")
                .await
                .unwrap();
            let outcome = tokage_orchestration::merge_task_worktree(
                &env,
                Path::new(&integration.root),
                &integration.branch,
                &worktree.branch,
                task,
                None,
            )
            .await
            .unwrap();
            assert!(matches!(
                outcome,
                tokage_orchestration::WorktreeMergeOutcome::Success { .. }
            ));
        }
        assert!(
            state
                .gate(
                    "npm test".into(),
                    &integration.cwd,
                    Some("gate:delivery:s"),
                    Some(config)
                )
                .unwrap()
                .passed
        );
        assert_eq!(std::fs::read_dir(&original).unwrap().count(), 0);
    }
    async fn demo_turn(mut req: chat::ChatRequest) -> (String, Vec<String>) {
        chat::demo::enforce_route(&mut req).unwrap();
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let registry = chat::TurnRegistry::new();
        registry.spawn_mock(bus, req);
        let mut raw = String::new();
        let mut files = vec![];
        let mut done = 0;
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                DomainEvent::ChatOutput {
                    event: EventKind::Text { text },
                    ..
                } => raw.push_str(&text),
                DomainEvent::ChatOutput {
                    event: EventKind::FileChange { path, .. },
                    ..
                } => files.push(path),
                DomainEvent::ChatOutput {
                    event:
                        EventKind::Done {
                            input_tokens,
                            output_tokens,
                            api_calls,
                            cost_usd,
                            ..
                        },
                    ..
                } => {
                    assert_eq!(
                        (input_tokens, output_tokens, api_calls, cost_usd),
                        (0, 0, Some(0), Some(0.0))
                    );
                    done += 1;
                }
                DomainEvent::ChatOutput {
                    event: EventKind::Error { message },
                    ..
                } => panic!("{message}"),
                DomainEvent::ChatEnded { cancelled, .. } => {
                    assert!(!cancelled);
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(done, 1);
        (raw, files)
    }

    #[tokio::test]
    async fn bilingual_demo_plans_run_with_real_git_reports_and_local_regressions() {
        for language in ["en", "zh"] {
            for scenario in [MockScenario::Success, MockScenario::Repair] {
                let dir = tempfile::tempdir().unwrap();
                let root = prepare_workspace(dir.path(), "tokage-demo:abcdef12").unwrap();
                assert!(Path::new(&root).ends_with("demo_project"));
                let config = DebugConfig {
                    output: DebugOutput::Mock,
                    demo: true,
                    demo_language: language.into(),
                    scenario,
                    ..Default::default()
                };
                let markdown = tokage_native::chat::demo::fixture()["locales"][language]["plan"]
                    ["rawMarkdown"]
                    .as_str()
                    .unwrap();
                let parsed = tokage_planning::parse_plan(markdown, &Default::default()).unwrap();
                let validation = tokage_planning::validate_plan(
                    parsed,
                    &tokage_planning::PlanValidationOptions {
                        allowed_commands: vec!["npm test".into()],
                        ..Default::default()
                    },
                );
                assert!(validation.is_valid, "{:?}", validation.issues);
                let plan = validation.plan;
                assert_eq!(plan.tasks.len(), 7);
                let allocation = tokage_matching::allocate_plan_models(
                    &plan,
                    &effective_pack(),
                    &tokage_matching::LearnedIndex::empty(),
                    None,
                    None,
                );
                assert_eq!(allocation.assignments.len(), 7);
                let env = ShellEnv {
                    vars: std::env::vars().collect(),
                };
                let integration = git::create_worktree(
                    &env,
                    Path::new(&root),
                    dir.path(),
                    "demo-integration",
                    None,
                )
                .await
                .unwrap();
                let state = DebugState::default();
                let scenario_name = if scenario == MockScenario::Repair {
                    "repair"
                } else {
                    "success"
                };
                let session = format!("tokage-demo-{scenario_name}-{language}-abcdef12");
                for (index, task) in plan.tasks.iter().enumerate() {
                    let worktree = git::create_worktree(
                        &env,
                        Path::new(&root),
                        dir.path(),
                        &format!("demo-{index}"),
                        Some(&integration.branch),
                    )
                    .await
                    .unwrap();
                    let mut req = chat::testing::request(ProviderId::ClaudeCode);
                    req.cwd = worktree.cwd.clone();
                    req.turn_id = format!("demo-{index}");
                    req.mode = TurnMode::Direct;
                    req.role = Some(if task.domain == "testing" {
                        Role::Tester
                    } else {
                        Role::Executor
                    });
                    req.session_id = Some(session.clone());
                    req.task_code = Some(task.id.clone());
                    req.task_id = Some(index as i64 + 1);
                    req.write_scope = task.write_scope.clone();
                    req.debug = Some(config.clone());
                    state.remember_request(&req, config.clone());
                    let (raw, files) = demo_turn(req.clone()).await;
                    assert_eq!(
                        files,
                        task.write_scope
                            .iter()
                            .map(|s| s.strip_suffix("/**").unwrap_or(s).to_string())
                            .collect::<Vec<_>>()
                    );
                    let report = tokage_orchestration::parse_or_generate_task_report(
                        task,
                        &raw,
                        "mock",
                        &req.turn_id,
                        true,
                        "1 file",
                        files,
                        "simulated",
                        Some(0.0),
                    );
                    assert!(!report.is_locally_generated);
                    assert!(report.contract_deviations.is_none());
                    assert!(report.defects.is_none());
                    for command in &task.acceptance.commands {
                        let run_id = format!("gate:{session}:{}", task.id);
                        let gate = state
                            .gate(command.clone(), &req.cwd, Some(&run_id), None)
                            .unwrap();
                        if scenario == MockScenario::Repair && task.id == "T2" {
                            assert!(!gate.passed);
                            req.phase = Some("REPAIR".into());
                            req.turn_id.push_str("-repair");
                            state.remember_request(&req, config.clone());
                            demo_turn(req.clone()).await;
                            assert!(
                                state
                                    .gate(command.clone(), &req.cwd, Some(&run_id), None)
                                    .unwrap()
                                    .passed
                            );
                        } else {
                            assert!(gate.passed);
                        }
                    }
                    git::commit_worktree(&env, Path::new(&worktree.root), "Demo task")
                        .await
                        .unwrap();
                    let outcome = tokage_orchestration::merge_task_worktree(
                        &env,
                        Path::new(&integration.root),
                        &integration.branch,
                        &worktree.branch,
                        task,
                        None,
                    )
                    .await
                    .unwrap();
                    assert!(matches!(
                        outcome,
                        tokage_orchestration::WorktreeMergeOutcome::Success { .. }
                    ));
                }
                // Unlike demo-mode gates, this regression command really runs, locally and without a model.
                let result = std::process::Command::new("node")
                    .arg("--test")
                    .current_dir(&integration.cwd)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{} {}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
                assert!(
                    !Path::new(&root).join("src/task-service.js").exists(),
                    "original demo checkout changes only on explicit delivery"
                );
            }
        }
    }
}
