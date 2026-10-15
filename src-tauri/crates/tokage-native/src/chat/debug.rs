//! Deterministic debug execution. Mock never probes or launches a model CLI.
//! Surrogates use one explicitly configured CLI and retain an opaque resume fingerprint.
use super::{ChatRequest, TimeoutLimits, TimeoutTier, TurnMode};
use crate::providers::ProviderInfo;
use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path},
    time::Duration,
};
use tokage_core::{Bus, Change, DomainEvent, EventKind, ProviderId, Role, ToolKind, UsageSource};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugOutput {
    #[default]
    Off,
    Mock,
    Luna,
    Deepseek,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MockScenario {
    #[default]
    Success,
    Repair,
    Error,
    Slow,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugConfig {
    pub output: DebugOutput,
    #[serde(default)]
    pub demo: bool,
    #[serde(default)]
    pub demo_language: String,
    #[serde(default)]
    pub scenario: MockScenario,
    #[serde(default)]
    pub chunk_delay_ms: u64,
    pub luna_provider: String,
    pub luna_model: String,
    pub deepseek_provider: String,
    pub deepseek_model: String,
    #[serde(default)]
    pub mock_reply: String,
}

impl DebugOutput {
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mock => "mock",
            Self::Luna => "luna",
            Self::Deepseek => "deepseek",
        }
    }
}

/// Strict routing: a missing target fails before any CLI can launch; no paid fallback.
pub fn route(req: &mut ChatRequest, providers: &[ProviderInfo]) -> Result<(), String> {
    let Some(config) = req.debug.clone().filter(|c| c.output != DebugOutput::Off) else {
        // Never feed a debug session id to a real CLI after debug is disabled.
        if req
            .resume
            .as_deref()
            .is_some_and(|s| s.starts_with("tokage-debug:"))
        {
            req.resume = None;
            req.fork = false;
        }
        return Ok(());
    };
    if config.output == DebugOutput::Mock {
        return Ok(());
    }
    let (provider, model) = match config.output {
        DebugOutput::Luna => (&config.luna_provider, &config.luna_model),
        DebugOutput::Deepseek => (&config.deepseek_provider, &config.deepseek_model),
        _ => unreachable!(),
    };
    let id: ProviderId = serde_json::from_value(serde_json::Value::String(provider.clone()))
        .map_err(|_| format!("Debug Provider: unknown target CLI {provider}"))?;
    if model.trim().is_empty() {
        return Err("Debug Provider: target model is empty".into());
    }
    if super::adapter::adapter(id).is_none() {
        return Err(format!("Debug Provider: {provider} has no chat adapter"));
    }
    let p = providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("Debug Provider: {provider} was not detected"))?;
    if !matches!(
        p.status,
        tokage_core::ProviderStatus::Ready | tokage_core::ProviderStatus::UnsupportedVersion
    ) || p.path.is_none()
    {
        return Err(format!(
            "Debug Provider: {provider} is not ready. Install/sign in to the configured CLI."
        ));
    }
    if !req.trusted.contains(&id) {
        return Err(format!(
            "Debug Provider: allow {provider} to work in this project under CLI management."
        ));
    }
    req.debug_requested = Some((req.provider, req.model.clone()));
    req.provider = id;
    req.model = model.trim().to_string();
    req.effort = None;
    req.fast = false;
    req.tier = None;
    req.resume = req
        .resume
        .as_deref()
        .and_then(|s| decode_resume(s, config.output, id, &req.model));
    if req.resume.is_none() {
        req.fork = false;
    }
    Ok(())
}

pub fn encode_resume(output: DebugOutput, provider: ProviderId, model: &str, id: &str) -> String {
    format!(
        "tokage-debug:{}",
        serde_json::json!([output.name(), provider.as_str(), model, id])
    )
}

fn decode_resume(
    encoded: &str,
    output: DebugOutput,
    provider: ProviderId,
    model: &str,
) -> Option<String> {
    let value: Vec<String> = serde_json::from_str(encoded.strip_prefix("tokage-debug:")?).ok()?;
    (value.len() == 4
        && value[0] == output.name()
        && value[1] == provider.as_str()
        && value[2] == model)
        .then(|| value[3].clone())
}

/// Terminal takeover must use the actual target and native id; mock has no real CLI.
pub fn terminal_target(encoded: &str) -> Option<(ProviderId, String)> {
    let value: Vec<String> = serde_json::from_str(encoded.strip_prefix("tokage-debug:")?).ok()?;
    if value.len() != 4 || !matches!(value[0].as_str(), "luna" | "deepseek") {
        return None;
    }
    let provider = serde_json::from_value(serde_json::json!(value[1])).ok()?;
    Some((provider, value[3].clone()))
}

pub fn route_event(req: &ChatRequest) -> EventKind {
    let (requested_provider, requested_model) = req
        .debug_requested
        .clone()
        .unwrap_or((req.provider, req.model.clone()));
    let output = req
        .debug
        .as_ref()
        .map(|c| c.output)
        .unwrap_or(DebugOutput::Off);
    EventKind::DebugRoute {
        output: output.name().into(),
        requested_provider: requested_provider.as_str().into(),
        requested_model,
        actual_provider: if output == DebugOutput::Mock {
            "DEBUG_PROVIDER".into()
        } else {
            req.provider.as_str().into()
        },
        workdir: req.cwd.clone(),
        actual_model: if output == DebugOutput::Mock {
            "mock".into()
        } else {
            req.model.clone()
        },
    }
}

pub const MOCK_PLAN: &str = include_str!("../../../../resources/debug/plan.md");

/// Marker is tracked in the fixture repo and survives real linked worktree creation.
pub fn is_mock_workspace(cwd: &Path) -> bool {
    cwd.ancestors()
        .any(|p| p.join(".tokage-debug-workspace").is_file())
}

fn mock_reply(req: &ChatRequest, config: &DebugConfig) -> String {
    if config.demo {
        return super::demo::reply(req, config);
    }
    if req.mode == TurnMode::Plan || req.effective_role() == Role::Planner {
        return MOCK_PLAN.into();
    }
    if req.task_id.is_some() || req.timeout_tier == Some(TimeoutTier::Agent) {
        let task = req
            .prompt
            .lines()
            .find_map(|l| l.strip_prefix("- Title: "))
            .unwrap_or("Scripted task");
        return format!("### What was completed\n{task} (simulated).\n### Deviations from contracts\nNone\n### Hints for downstream\nUse the deterministic debug result.\n### Self-check against acceptance criteria\n- Scripted acceptance feedback is ready.\n### Defects found\nNone\n");
    }
    if !config.mock_reply.trim().is_empty() {
        return config.mock_reply.clone();
    }
    format!("Debug Provider returned a scripted response.\n\nSelected model: `{}` / `{}`.\n\nNo model API was called. Switch the output source in Debug Provider settings to use Luna or DeepSeek.", req.provider, req.model)
}

fn write_mock_file(req: &ChatRequest, emit: &impl Fn(EventKind)) -> Result<(), String> {
    if !req.effective_role().allows_writes() {
        return Ok(());
    }
    let Some(relative) = req
        .write_scope
        .iter()
        // The production validator normalizes literal paths to `<path>/**`.
        // Recover a concrete file, while leaving directory/glob scopes untouched.
        .map(|s| s.strip_suffix("/**").unwrap_or(s))
        .find(|s| !s.contains(['*', '?', '[']) && Path::new(s).extension().is_some())
    else {
        return Ok(());
    };
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Debug Provider: write scope must be a relative path".into());
    }
    let cwd = Path::new(&req.cwd)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !is_mock_workspace(&cwd) {
        return Err("Debug Provider: mock writes require an isolated debug workspace".into());
    }
    // Reject symlinks in every existing path component before creating anything.
    let mut target = cwd.clone();
    for part in relative_path.components() {
        target.push(part.as_os_str());
        if target
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("Debug Provider: symlink write scope is not supported".into());
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let before = std::fs::read_to_string(&target).ok();
    let demo_content = req
        .debug
        .as_ref()
        .filter(|c| c.demo)
        .and_then(|c| super::demo::file(req, c, relative));
    let after = if let Some(content) = demo_content.as_deref() {
        content
    } else if req.effective_phase() == "REPAIR" {
        "export const debugResult = 'repaired';\n"
    } else {
        "export const debugResult = 'success';\n"
    };
    if before.as_deref() == Some(after) {
        return Ok(());
    }
    std::fs::write(&target, after).map_err(|e| e.to_string())?;
    emit(EventKind::ToolStart {
        id: "mock-edit".into(),
        tool: ToolKind::Edit,
        title: relative.to_string(),
        detail: Some(
            req.debug
                .as_ref()
                .filter(|c| c.demo)
                .map(|c| super::demo::text(c, "working").to_string())
                .unwrap_or_else(|| "Simulated edit in the isolated debug workspace".into()),
        ),
    });
    emit(EventKind::ToolEnd {
        id: "mock-edit".into(),
        ok: Some(true),
        output: None,
    });
    let text_diff = similar::TextDiff::from_lines(before.as_deref().unwrap_or(""), after);
    let additions = text_diff
        .iter_all_changes()
        .filter(|c| c.tag() == similar::ChangeTag::Insert)
        .count() as u32;
    let deletions = text_diff
        .iter_all_changes()
        .filter(|c| c.tag() == similar::ChangeTag::Delete)
        .count() as u32;
    let diff = text_diff
        .unified_diff()
        .header(&format!("a/{relative}"), &format!("b/{relative}"))
        .to_string();
    emit(EventKind::FileChange {
        tool_id: Some("mock-edit".into()),
        path: relative.to_string(),
        abs_path: target.display().to_string(),
        preview_path: None,
        change: if before.is_some() {
            Change::Modified
        } else {
            Change::Added
        },
        diff: Some(diff),
        additions,
        deletions,
    });
    Ok(())
}

pub async fn run_mock(
    bus: &Bus,
    req: &ChatRequest,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> bool {
    let config = req.debug.as_ref().expect("mock config");
    let emit = |event| {
        bus.publish(DomainEvent::ChatOutput {
            turn_id: req.turn_id.clone(),
            event,
        })
    };
    emit(route_event(req));
    emit(EventKind::Thread {
        id: encode_resume(DebugOutput::Mock, req.provider, &req.model, &req.turn_id),
    });
    emit(EventKind::Model {
        name: "mock".into(),
    });
    let limits = TimeoutLimits::resolved(
        req.timeout_tier.unwrap_or(TimeoutTier::Standard),
        req.timeout_multiplier.unwrap_or(1.0),
        req.idle_timeout_secs,
        req.hard_timeout_secs,
    );
    let pause = |duration| async move {
        tokio::time::timeout(limits.idle, tokio::time::sleep(duration))
            .await
            .map_err(|_| "Debug Provider: idle timeout".to_string())
    };
    let body = async {
        if config.scenario == MockScenario::Slow
            && (!config.demo || req.task_code.as_deref() == Some("T3"))
        {
            pause(Duration::from_secs(30)).await?;
        }
        if config.scenario == MockScenario::Error
            && (!config.demo || req.task_code.as_deref() == Some("T3"))
            && req.effective_role() != Role::Planner
        {
            return Err(if config.demo {
                super::demo::text(config, "failure").into()
            } else {
                "Debug Provider: scripted execution failure".to_string()
            });
        }
        write_mock_file(req, &emit)?;
        let reply = mock_reply(req, config);
        let chars: Vec<char> = reply.chars().collect();
        for chunk in chars.chunks(32) {
            pause(Duration::from_millis(config.chunk_delay_ms.min(5000))).await?;
            emit(EventKind::Text {
                text: chunk.iter().collect(),
            });
        }
        Ok::<(), String>(())
    };
    let result = tokio::select! {
        biased;
        _ = async { loop { if *cancel.borrow_and_update() { break; } if cancel.changed().await.is_err() { break; } } } => return true,
        result = tokio::time::timeout(limits.hard, body) => result.unwrap_or_else(|_| Err("Debug Provider: hard timeout".into())),
    };
    match result {
        Ok(()) => emit(EventKind::Done {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            api_calls: Some(0),
            context_tokens: Some(0),
            cost_usd: Some(0.0),
            premium_requests: Some(0.0),
            credits: None,
            source: UsageSource::Structured,
        }),
        Err(message) => emit(EventKind::Error { message }),
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_is_scoped_to_source_provider_and_model() {
        let s = encode_resume(DebugOutput::Luna, ProviderId::CodexCli, "luna", "native:id");
        assert_eq!(
            decode_resume(&s, DebugOutput::Luna, ProviderId::CodexCli, "luna"),
            Some("native:id".into())
        );
        assert_eq!(
            decode_resume(&s, DebugOutput::Deepseek, ProviderId::CodexCli, "luna"),
            None
        );
        assert_eq!(
            decode_resume(&s, DebugOutput::Luna, ProviderId::CursorCli, "luna"),
            None
        );
        assert_eq!(
            decode_resume(&s, DebugOutput::Luna, ProviderId::CodexCli, "other"),
            None
        );
        assert_eq!(
            decode_resume("real-id", DebugOutput::Luna, ProviderId::CodexCli, "luna"),
            None
        );
        assert_eq!(
            terminal_target(&s),
            Some((ProviderId::CodexCli, "native:id".into()))
        );
        assert_eq!(
            terminal_target(&encode_resume(
                DebugOutput::Mock,
                ProviderId::ClaudeCode,
                "opus",
                "id"
            )),
            None
        );
    }
    #[tokio::test]
    async fn mock_has_zero_usage_and_preserves_unicode() {
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let mut req = super::super::testing::request(ProviderId::ClaudeCode);
        req.debug = Some(DebugConfig {
            output: DebugOutput::Mock,
            mock_reply: "中文流式输出🦎".repeat(30),
            ..Default::default()
        });
        let (_tx, cancel) = tokio::sync::watch::channel(false);
        assert!(!run_mock(&bus, &req, cancel).await);
        let mut text = String::new();
        let mut done = 0;
        while let Ok(event) = rx.try_recv() {
            if let DomainEvent::ChatOutput { event, .. } = event {
                match event {
                    EventKind::Text { text: chunk } => text.push_str(&chunk),
                    EventKind::Done {
                        input_tokens,
                        output_tokens,
                        api_calls,
                        ..
                    } => {
                        assert_eq!((input_tokens, output_tokens, api_calls), (0, 0, Some(0)));
                        done += 1;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(text, req.debug.as_ref().unwrap().mock_reply);
        assert_eq!(done, 1);
    }

    #[tokio::test]
    async fn cancellation_and_failure_end_once_without_success_or_health_changes() {
        for scenario in [MockScenario::Slow, MockScenario::Error] {
            let bus = Bus::new();
            let mut rx = bus.subscribe();
            let registry = super::super::TurnRegistry::new();
            let mut req = super::super::testing::request(ProviderId::ClaudeCode);
            req.debug = Some(DebugConfig {
                output: DebugOutput::Mock,
                scenario,
                ..Default::default()
            });
            registry.spawn_mock(bus, req.clone());
            if scenario == MockScenario::Slow {
                registry.cancel(&req.turn_id).await;
            }
            let mut ended = 0;
            let mut error = 0;
            loop {
                let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await
                    .unwrap()
                    .unwrap();
                match event {
                    DomainEvent::ChatOutput {
                        event: EventKind::Error { .. },
                        ..
                    } => error += 1,
                    DomainEvent::ChatOutput {
                        event: EventKind::Done { .. },
                        ..
                    } => panic!("cancel/failure must not report success"),
                    DomainEvent::TurnFailed { .. } | DomainEvent::TurnSucceeded { .. } => {
                        panic!("mock must not alter real provider health")
                    }
                    DomainEvent::ChatEnded { cancelled, .. } => {
                        assert_eq!(cancelled, scenario == MockScenario::Slow);
                        ended += 1;
                        break;
                    }
                    _ => {}
                }
            }
            while let Ok(event) = rx.try_recv() {
                if matches!(event, DomainEvent::ChatEnded { .. }) {
                    ended += 1;
                }
            }
            assert_eq!(ended, 1);
            assert_eq!(error, usize::from(scenario == MockScenario::Error));
        }
    }

    #[tokio::test]
    async fn mock_timeout_is_reported_without_waiting_for_the_script() {
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let mut req = super::super::testing::request(ProviderId::CodexCli);
        req.timeout_tier = Some(TimeoutTier::Quick);
        req.timeout_multiplier = Some(0.001);
        req.debug = Some(DebugConfig {
            output: DebugOutput::Mock,
            scenario: MockScenario::Slow,
            ..Default::default()
        });
        let (_tx, cancel) = tokio::sync::watch::channel(false);
        assert!(!run_mock(&bus, &req, cancel).await);
        let mut timeout = false;
        while let Ok(ev) = rx.try_recv() {
            if let DomainEvent::ChatOutput {
                event: EventKind::Error { message },
                ..
            } = ev
            {
                timeout |= message.contains("timeout");
            }
        }
        assert!(timeout);
    }

    fn ready(id: ProviderId) -> ProviderInfo {
        ProviderInfo {
            id,
            name: "Test CLI",
            executable: "never-run",
            featured: true,
            status: tokage_core::ProviderStatus::Ready,
            version: None,
            path: Some("/never-run".into()),
            auth_method: None,
            models: vec![],
            model_groups: vec![],
            models_source: None,
            account: None,
            guidance: vec![],
            install_methods: vec![],
            trust_args: vec![],
            chat: true,
            adapter: None,
            direct_limit: None,
        }
    }

    #[test]
    fn surrogate_uses_only_configured_cli_and_never_substitutes_another_model() {
        let mut req = super::super::testing::request(ProviderId::ClaudeCode);
        req.model = "opus".into();
        req.effort = Some("high".into());
        req.fast = true;
        req.debug = Some(DebugConfig {
            output: DebugOutput::Deepseek,
            deepseek_provider: "OPENCODE".into(),
            deepseek_model: "deepseek/custom".into(),
            ..Default::default()
        });
        assert!(route(&mut req, &[]).is_err());
        assert_eq!(req.provider, ProviderId::ClaudeCode);
        assert!(
            route(&mut req, &[ready(ProviderId::Opencode)]).is_err(),
            "target requires explicit trust"
        );
        req.trusted = vec![ProviderId::Opencode];
        route(&mut req, &[ready(ProviderId::Opencode)]).unwrap();
        assert_eq!(req.provider, ProviderId::Opencode);
        assert_eq!(req.model, "deepseek/custom");
        assert_eq!(
            req.debug_requested,
            Some((ProviderId::ClaudeCode, "opus".into()))
        );
        assert_eq!(req.effort, None);
        assert!(!req.fast);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn surrogate_runs_normal_cli_parser_and_preserves_route_and_resume_provenance() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("scripted-codex");
        std::fs::write(&exe, "#!/bin/sh\n/bin/cat >/dev/null\n/bin/cat <<'EVENTS'\n{\"type\":\"thread.started\",\"thread_id\":\"real-native-id\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"reply\",\"type\":\"agent_message\",\"text\":\"surrogate stream\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}\nEVENTS\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut provider = ready(ProviderId::CodexCli);
        provider.path = Some(exe.display().to_string());
        let mut req = super::super::testing::request(ProviderId::ClaudeCode);
        req.cwd = dir.path().display().to_string();
        req.model = "opus".into();
        req.trusted = vec![ProviderId::CodexCli];
        req.debug = Some(DebugConfig {
            output: DebugOutput::Luna,
            luna_provider: "CODEX_CLI".into(),
            luna_model: "gpt-6-luna".into(),
            ..Default::default()
        });
        route(&mut req, &[provider.clone()]).unwrap();
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        let env = crate::env::ShellEnv {
            vars: std::env::vars().collect(),
        };
        assert!(!super::super::run_turn(&bus, req, vec![provider], env, None, None).await);
        let mut reply = String::new();
        let mut routed = false;
        let mut thread = false;
        let mut succeeded = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                DomainEvent::ChatOutput {
                    event:
                        EventKind::DebugRoute {
                            requested_provider,
                            requested_model,
                            actual_provider,
                            actual_model,
                            ..
                        },
                    ..
                } => {
                    assert_eq!(
                        (requested_provider.as_str(), requested_model.as_str()),
                        ("CLAUDE_CODE", "opus")
                    );
                    assert_eq!(
                        (actual_provider.as_str(), actual_model.as_str()),
                        ("CODEX_CLI", "gpt-6-luna")
                    );
                    routed = true;
                }
                DomainEvent::ChatOutput {
                    event: EventKind::Thread { id },
                    ..
                } => {
                    assert_eq!(
                        decode_resume(&id, DebugOutput::Luna, ProviderId::CodexCli, "gpt-6-luna"),
                        Some("real-native-id".into())
                    );
                    thread = true;
                }
                DomainEvent::ChatOutput {
                    event: EventKind::Text { text },
                    ..
                } => reply.push_str(&text),
                DomainEvent::TurnSucceeded {
                    provider: ProviderId::CodexCli,
                } => succeeded = true,
                DomainEvent::ChatOutput {
                    event: EventKind::Error { message },
                    ..
                } => panic!("{message}"),
                _ => {}
            }
        }
        assert!(routed && thread && succeeded);
        assert_eq!(reply, "surrogate stream");
    }
}
