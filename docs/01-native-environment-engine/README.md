# 01 · Native Environment Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

1. Detect officially installed AI CLIs on the machine; obtain version, login state, and available models.
2. Wrap each CLI in a unified `ProviderAdapter` for invocation, resume, compact, and permission parameters.
3. Start CLIs with the correct permissions per task role (planning / execution / test), headless by default, PTY as fallback.
4. Produce accurate per-turn usage (including cache read/write) and structured tool events.
5. Read-only parse each CLI’s local session files for zero-token native session inspection.

## 2. Non-Intrusive Principle

- Do not write aliases, functions, or rc files; do not modify CLI config files.
- All invocations use official CLI flags, subcommands, and slash commands.
- When reading CLI local data: open read-only, no locking; tolerate concurrent writes (drop incomplete last line).
- In the project directory Tokage writes under `.tokage/` and maintains references in `AGENTS.md` / `CLAUDE.md`; `.gitignore` is changed only with user consent (see [08 §1.1](../08-storage/README.md)).

## 3. Environment Variable Capture

On macOS, GUI apps started by launchd do not get PATH from `.zshrc`. On startup run once:

```
$SHELL -ilc 'env -0'
```

- 5s timeout; on failure fall back to current process env + common paths (`/opt/homebrew/bin`, `~/.local/bin`, `~/.npm-global/bin`).
- Result kept in memory only (may contain API keys); user can “Reload environment” in settings.
- When spawning children, not passed through verbatim today: trim vendor credential vars per CLI, inject proxy vars, strip host session vars ([11 §6.3](../11-command-authorization/README.md), [12](../12-network-proxy/README.md)). Current behavior: pass through as-is.

## 4. Provider Registry

| ProviderId | Executable | Headless | Structured output | Resume | Native compact | Read-only mode |
|-----------|-----------|----------|-----------|------|---------|---------|
| `CLAUDE_CODE` | `claude` | `claude -p` | `--output-format stream-json` | `--resume <id>` | `/compact [instruction]` | `--permission-mode plan` or read-only tool allowlist |
| `CODEX_CLI` | `codex` | `codex exec` | `--json` | `exec resume <id>` | `/compact` | `--sandbox read-only` |
| `CURSOR_CLI` | `cursor-agent` | `cursor-agent -p` | `--output-format stream-json --stream-partial-output` | `--resume <id>` | TBD | `--mode plan` |
| `COPILOT_CLI` | `copilot` | `copilot -p` | `--output-format json` | `--resume <id>` | TBD | `--plan` + `--deny-tool write/shell` |
| `ANTIGRAVITY_CLI` | `agy` | `agy --headless` | `--output-format stream-json` | `--resume <id>` | TBD | `--mode plan` or `--disallowedTools ...` |
| `AIDER` | `aider` | `aider --message` | None (parse trailing `Tokens: ... Cost: ...` line) | `--restore-chat-history` | Built-in auto summary | `--dry-run` / `--no-git` |
| `OPENCODE` | `opencode` | `opencode run` | `--format json` | `run --resume <id>` | TBD | `--mode plan` or `--deny-tools ...` |
| `REASONIX` | `reasonix` | `reasonix run` | `--output-format stream-json` | `--resume <id>` (`--copy` can fork) | `~/.reasonix/projects/*/sessions` | `--permission-mode read-only` |

- Headless Adapters implemented: Claude Code, Codex CLI, Cursor CLI, Copilot CLI, Antigravity CLI, Aider, OpenCode, Reasonix. Other allowlisted CLIs (Kiro, Qwen Code, etc.; see `src-tauri/crates/tokage-native/src/providers.rs`) are detection-only for now.
- Reasonix defaults `featured: false` (add via Settings “Add CLI” dropdown); DeepSeek etc. can still route through Aider / OpenCode.
- Each Adapter declares supported version range in metadata and regression-tests with recorded samples (§10). Outside range still attempts run, marks `UNSUPPORTED_VERSION` (only when previously `READY`; login issues take precedence); CLI panel shows tested range; if a turn has no recognizable structured events, non-JSON CLI output (cap 64 KiB) is shown as reply text.

## 5. ProviderAdapter Abstraction

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;
    fn meta(&self) -> AdapterMeta;   // executable name, version range, capability bits

    async fn detect(&self, env: &ShellEnv) -> DetectResult;              // within 10s
    fn build_turn(&self, req: &TurnRequest) -> CommandSpec;              // no shell string concat
    fn build_compact(&self, native_id: &str, focus: Option<&str>) -> Option<CommandSpec>;
    fn parse_line(&self, line: &str, state: &mut ParseState) -> Vec<AdapterEvent>;
    async fn read_native_sessions(&self, project: &Path) -> Result<Vec<NativeSession>>;
    async fn query_usage(&self, env: &ShellEnv) -> Option<UsageSnapshot>; // only when query does not consume quota
}

pub struct TurnRequest {
    pub cwd: PathBuf,
    pub model: Option<String>,        // verbatim string for --model
    pub input: String,                // task brief or follow-up message
    pub resume_id: Option<String>,    // None = new window
    pub role: AgentRole,              // PLANNER | EXECUTOR | TESTER | REVIEWER
    pub write_scope: Vec<GlobPattern>,// allowed write scope for EXECUTOR / TESTER (see 09)
    pub timeout_class: TimeoutClass,
}

pub enum AdapterEvent {
    SessionId(String),
    AssistantText(String),
    ToolCall { name: String, summary: String, touched_paths: Vec<PathBuf> },
    ToolResult { summary: String, exit_code: Option<i32> },
    TurnUsage(TurnUsage),
    RateLimited { raw: String },
    AuthRequired,
    Error(String),
    Done { success: bool },
}

pub struct TurnUsage {
    pub input_tokens: u64,            // input not served from cache
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub api_calls: Option<u32>,       // API requests this turn (tool loop)
    pub context_tokens: Option<u64>,  // context size of last request this turn
    pub cost_usd: Option<f64>,
    pub source: UsageSource,          // STRUCTURED | STDOUT_REGEX | ESTIMATED
}
```

Capability bits `AdapterMeta.caps`: `STRUCTURED_OUTPUT | RESUME | MODEL_SELECT | READ_ONLY_MODE | WRITE_SCOPE | COMPACT_HEADLESS | COMPACT_PTY | USAGE_QUERY | NATIVE_SESSIONS | NATIVE_FORK`, plus authorization and sandbox bits from [11 §4.1](../11-command-authorization/README.md). Upper engines depend only on capability bits.

Implementation (`src-tauri/crates/tokage-native/src/chat/adapter.rs`): trait currently includes `meta()`, `launch()` (args, stdin, stream parser for the turn) and `direct_limit()`; `detect` / `build_compact` / `read_native_sessions` / `query_usage` added per phase. `AdapterMeta` includes capability bits, tested version range (`min` inclusive, `below` exclusive), recorded sample version and filename. Bits mark only what Tokage uses and verified on tested versions:

| Adapter | Tested version | Range | Capability bits (besides `STRUCTURED_OUTPUT` / `RESUME` / `MODEL_SELECT` / `READ_ONLY_MODE`) |
|---------|---------|------|------|
| Claude Code | 2.1.288 | `>= 2.1.288, < 3` | `NATIVE_FORK`, `TOOL_RULES`, `NATIVE_SESSIONS` |
| Codex CLI | 0.160.0 | `>= 0.160.0, < 1` | `OWN_SANDBOX`, `NATIVE_SESSIONS` |
| Cursor CLI | 2026.10.01 | `>= 2026.10.01` | `DENIAL_EVENT`, `NATIVE_SESSIONS` |
| Copilot CLI | 1.0.91 | `>= 1.0.91, < 2` | `TOOL_RULES`, `NATIVE_SESSIONS` |
| Antigravity CLI | 0.1.0 | `>= 0.1.0, < 1` | `TOOL_RULES`, `SHELL_WRAP` |
| Aider | 0.75.0 | `>= 0.70.0, < 1` | Text stream (`STDOUT_REGEX`), no `STRUCTURED_OUTPUT` |
| OpenCode | 0.2.0 | `>= 0.2.0, < 1` | `TOOL_RULES` |

Capability bits and ranges are sent to the frontend via `ProviderInfo.adapter` (e.g. whether fork uses native fork).

## 6. Roles and Permissions

| Role | Permissions | Use |
|------|------|------|
| `PLANNER` | Read-only: read files, search; no write, no command execution | Planning; see [02](../02-planning-engine/README.md) |
| `EXECUTOR` | Write within task `write_scope`; run build/test commands | Development tasks |
| `TESTER` | Write test dirs; run test commands | Test tasks |
| `REVIEWER` | Read-only | Optional review tasks |

- Permissions expressed via official CLI permission / sandbox / tool allowlist flags, fixed when the execution window is created, unchanged within the window (also protects cache prefix).
- When a CLI lacks path-level write limits, [09 §4](../09-guardrails/README.md) validates actual change scope after the task.
- Tokage does not enable “skip all permission prompts” style flags for the user.
- Commands initiated by the model in execution roles are further handled by authorization mode and command lists ([11](../11-command-authorization/README.md)); Adapters declare available mechanisms via `APPROVAL_CALLBACK` / `TOOL_RULES` / `DENIAL_EVENT` / `SHELL_WRAP` / `OS_SANDBOX`.

## 7. Execution Modes

### 7.1 Headless (default)

- `tokio::process::Command`; input via args or one-shot stdin; `stdout` / `stderr` piped line-by-line (1 MiB max per line).
- Each line passes output filter ([09 §1](../09-guardrails/README.md)) then `parse_line`.
- One process per turn; multi-turn via `resume_id`; Prompt Cache applies to resumed turns. No `resume_id` = new window.

### 7.2 PTY (fallback)

For CLIs without headless support, or slash commands that only work interactively.

- `portable-pty` fixed size (200×50); enter target session with resume args.
- Output parsed with `vte` into virtual screen; snapshot on “stable frame” (300ms unchanged).
- End on completion prompt match, process exit, or timeout; exit immediately after, no long-lived PTY.

## 8. Detection and Native Session Inspection

Detection:

```
refresh_providers()
  for adapter in registry (concurrent, 10s timeout each):
     1. which(exe)          → not found: NOT_INSTALLED
     2. exe --version       → outside range: UNSUPPORTED_VERSION (still usable)
     3. Login (check only): CLI status command > credential file / keychain entry exists (no content read) > UNKNOWN_AUTH
     4. Model list: CLI list command > models for that Provider in definition pack
  write provider_health, emit ProviderDetected
```

`ProviderStatus`: `READY | NOT_INSTALLED | NEEDS_LOGIN | UNSUPPORTED_VERSION | UNKNOWN_AUTH | DEGRADED | MELTED`. When login needed, prompt user to run official login command and offer “Open in terminal”.

Native session inspection (read-only, zero token):

| Provider | Data location (per Adapter supported versions) | Format |
|----------|-----------------------------------|------|
| Claude Code | `~/.claude/projects/<encoded-project-path>/*.jsonl` | JSONL |
| Codex CLI | `~/.codex/sessions/**` | JSONL |
| Cursor CLI / Copilot CLI / Antigravity CLI | TBD | TBD |
| Aider | Project root `.aider.chat.history.md` | Markdown |

`notify` watches directories for incremental reads, mapped to `NativeSession`. Frontend sidebar `ExternalSessionsSection` (F17.a) calls `listNativeSessions` read-only for vendor badge, title, updated time (filtered by current project cwd, 30s refresh). On parse failure, degrade to filename and mtime only.

## 9. Error Classification

| Category | Handling |
|------|------|
| `SpawnFailed` | Re-detect, set NOT_INSTALLED |
| `AuthRequired` | Set NEEDS_LOGIN; queued tasks re-assigned by matching engine |
| `RateLimited` | Hand off to UsageEngine melting |
| `Timeout` | Reap process group; orchestration repair strategy |
| `ParseError` | Fall back to plain text log; usage locally estimated; flag Adapter for update |
| `NonZeroExit` | Log last 4KB of stderr |

## 10. Test Strategy

- Recorded output samples per Adapter; snapshot tests for `parse_line` covering usage, cache, tool events.
- `FakeAdapter` (`chat/fake.rs`, tests only): `/bin/sh` script impersonates a CLI, same `run_cli` path as real turns (process group, env trim, file tracking). Can replay samples, inject delay, hang, spam, child processes, out-of-scope writes, credential path reads, env dumps, specified success/failure for upper-engine tests.
- Real CLIs: default suite includes non-`#[ignore]` `chat::live::smoke_detect_skip_if_absent` (detect + `executable`, skip pass if CLI absent; full cheap turn when `TOKAGE_LIVE_SMOKE=1`). Full turns remain `#[ignore]`. Entry: `scripts/run-live-cli-e2e.sh`.
