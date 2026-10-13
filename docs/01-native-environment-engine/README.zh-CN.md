# 01 · 原生环境与联邦引擎 (Native Environment Engine)

[English](README.md) | **中文**

返回 [README](../../README.zh-CN.md)

## 1. 职责

1. 探测本机已安装的官方 AI CLI，获取版本、登录态、可用模型。
2. 以统一的 `ProviderAdapter` 封装每个 CLI 的调用、续接、压缩与权限参数。
3. 按任务角色（规划 / 执行 / 测试）以正确的权限启动 CLI，headless 为主、PTY 兜底。
4. 产出精确的逐轮用量（含缓存读写）与结构化工具事件。
5. 只读解析各 CLI 的本地会话文件，实现零 Token 的原版会话透视。

## 2. 无侵入原则

- 不写入 alias、函数或 rc 文件；不修改 CLI 的配置文件。
- 所有调用使用 CLI 的官方参数、子命令与斜杠命令。
- 读取 CLI 本地数据时只读打开、不加锁，容忍并发写入（丢弃不完整的末行）。
- 在项目目录中，Tokage 写 `.tokage/` 目录，另在 `AGENTS.md` / `CLAUDE.md` 中维护引用；`.gitignore` 仅在用户同意后改动（见 [08 §1.1](../08-storage/README.zh-CN.md)）。

## 3. 环境变量获取

GUI 应用在 macOS 上由 launchd 启动，拿不到 `.zshrc` 中配置的 PATH。启动时执行一次：

```
$SHELL -ilc 'env -0'
```

- 超时 5 秒；失败回退为当前进程环境 + 常见路径（`/opt/homebrew/bin`、`~/.local/bin`、`~/.npm-global/bin`）。
- 结果只保存在内存中（可能含 API Key），用户可在设置中「重新加载环境」。
- 启动子进程时不再原样透传：按 CLI 裁剪厂商凭据变量、注入代理变量、移除宿主会话变量（[11 §6.3](../11-command-authorization/README.zh-CN.md)、[12](../12-network-proxy/README.zh-CN.md)）。现状为原样透传。

## 4. Provider 注册表

| ProviderId | 可执行文件 | Headless | 结构化输出 | 续接 | 原生压缩 | 只读模式 |
|-----------|-----------|----------|-----------|------|---------|---------|
| `CLAUDE_CODE` | `claude` | `claude -p` | `--output-format stream-json` | `--resume <id>` | `/compact [指示]` | `--permission-mode plan` 或只读工具允许列表 |
| `CODEX_CLI` | `codex` | `codex exec` | `--json` | `exec resume <id>` | `/compact` | `--sandbox read-only` |
| `CURSOR_CLI` | `cursor-agent` | `cursor-agent -p` | `--output-format stream-json --stream-partial-output` | `--resume <id>` | 待确认 | `--mode plan` |
| `COPILOT_CLI` | `copilot` | `copilot -p` | `--output-format json` | `--resume <id>` | 待确认 | `--plan` + `--deny-tool write/shell` |
| `ANTIGRAVITY_CLI` | `agy` | `agy -p` | `--output-format stream-json` | `--conversation <id>` | 待确认 | `--mode plan` + 只读工作区 OS 沙箱 |
| `AIDER` | `aider` | `aider --message` | 无（解析末尾 `Tokens: ... Cost: ...` 行） | `--restore-chat-history` | 内置自动摘要 | `--dry-run` / `--no-git` |
| `OPENCODE` | `opencode` | `opencode run` | `--format json` | `run --resume <id>` | 待确认 | `--mode plan` 或 `--deny-tools ...` |
| `REASONIX` | `reasonix` | `reasonix run` | `--output-format stream-json` | `--resume <id>`（`--copy` 可 fork） | `~/.reasonix/projects/*/sessions` | `--permission-mode read-only` |

- 已实现 headless Adapter：Claude Code、Codex CLI、Cursor CLI、Copilot CLI、Antigravity CLI、Aider、OpenCode、Reasonix。其余白名单 CLI（Kiro、Qwen Code 等，见 `src-tauri/crates/tokage-native/src/providers.rs`）目前做探测。
- Reasonix 默认 `featured: false`（设置页「添加 CLI」下拉加入）；DeepSeek 等也可仍经 Aider / OpenCode 接入。
- 每个 Adapter 在元数据中声明支持的版本区间，并以录制样本做回归测试（§10）。版本超出区间时仍尝试运行，标记 `UNSUPPORTED_VERSION`（仅在原本为 `READY` 时；未登录等状态优先），CLI 面板显示已测区间；一轮输出中没有可识别的结构化事件时，CLI 打印的非 JSON 内容（上限 64 KiB）作为回复文本展示。

## 5. ProviderAdapter 抽象

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;
    fn meta(&self) -> AdapterMeta;   // 可执行名、版本区间、能力位

    async fn detect(&self, env: &ShellEnv) -> DetectResult;              // 10s 内返回
    fn build_turn(&self, req: &TurnRequest) -> CommandSpec;              // 不经 shell 拼接
    fn build_compact(&self, native_id: &str, focus: Option<&str>) -> Option<CommandSpec>;
    fn parse_line(&self, line: &str, state: &mut ParseState) -> Vec<AdapterEvent>;
    async fn read_native_sessions(&self, project: &Path) -> Result<Vec<NativeSession>>;
    async fn query_usage(&self, env: &ShellEnv) -> Option<UsageSnapshot>; // 仅当查询不耗额度
}

pub struct TurnRequest {
    pub cwd: PathBuf,
    pub model: Option<String>,        // 传给 --model 的原样字符串
    pub input: String,                // 任务简报或追加消息
    pub resume_id: Option<String>,    // None 即新窗口
    pub role: AgentRole,              // PLANNER | EXECUTOR | TESTER | REVIEWER
    pub write_scope: Vec<GlobPattern>,// EXECUTOR / TESTER 的允许写入范围（见 09）
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
    pub input_tokens: u64,            // 未命中缓存的输入
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub api_calls: Option<u32>,       // 本轮 API 请求次数（工具循环）
    pub context_tokens: Option<u64>,  // 本轮最后一次请求的上下文总量
    pub cost_usd: Option<f64>,
    pub source: UsageSource,          // STRUCTURED | STDOUT_REGEX | ESTIMATED
}
```

能力位 `AdapterMeta.caps`：`STRUCTURED_OUTPUT | RESUME | MODEL_SELECT | READ_ONLY_MODE | WRITE_SCOPE | COMPACT_HEADLESS | COMPACT_PTY | USAGE_QUERY | NATIVE_SESSIONS | NATIVE_FORK`，加上 [11 §4.1](../11-command-authorization/README.zh-CN.md) 的授权与沙箱能力位。上层引擎只依赖能力位。

实现（`src-tauri/crates/tokage-native/src/chat/adapter.rs`）：trait 目前包含 `meta()`、`launch()`（参数、stdin 与本轮的流解析器）与 `direct_limit()`；`detect` / `build_compact` / `read_native_sessions` / `query_usage` 随所属阶段加入。`AdapterMeta` 含能力位、已测版本区间（`min` 含、`below` 不含）、录制样本的版本与文件名。能力位只标注 Tokage 已使用并在已测版本上验证过的：

| Adapter | 已测版本 | 区间 | 能力位（除 `STRUCTURED_OUTPUT` / `RESUME` / `MODEL_SELECT` / `READ_ONLY_MODE`） |
|---------|---------|------|------|
| Claude Code | 2.1.288 | `>= 2.1.288, < 3` | `NATIVE_FORK`、`TOOL_RULES`、`NATIVE_SESSIONS` |
| Codex CLI | 0.160.0 | `>= 0.160.0, < 1` | `OWN_SANDBOX`、`NATIVE_SESSIONS` |
| Cursor CLI | 2026.10.01 | `>= 2026.10.01` | `DENIAL_EVENT`、`NATIVE_SESSIONS` |
| Copilot CLI | 1.0.91 | `>= 1.0.91, < 2` | `TOOL_RULES`、`NATIVE_SESSIONS` |
| Antigravity CLI | 1.3.4 | `>= 1.3.4, < 2` | `OS_SANDBOX`、`NATIVE_SESSIONS`、`USAGE_QUERY` |
| Aider | 0.75.0 | `>= 0.70.0, < 1` | 文本流（`STDOUT_REGEX`）、无 `STRUCTURED_OUTPUT` |
| OpenCode | 0.2.0 | `>= 0.2.0, < 1` | `TOOL_RULES` |

能力位与区间经 `ProviderInfo.adapter` 传给前端（如分叉会话是否走原生 fork）。

## 6. 角色与权限

| 角色 | 权限 | 用途 |
|------|------|------|
| `PLANNER` | 只读：可读文件、搜索；不可写、不可执行命令 | 规划，见 [02](../02-planning-engine/README.zh-CN.md) |
| `EXECUTOR` | 可写，限定在任务的 `write_scope`；可执行构建 / 测试类命令 | 开发任务 |
| `TESTER` | 可写测试目录；可执行测试命令 | 测试任务 |
| `REVIEWER` | 只读 | 可选的评审任务 |

- 权限通过 CLI 官方的权限 / 沙箱 / 工具允许列表参数表达，在执行窗口创建时固定，窗口内不再改变（同时保护缓存前缀）。
- CLI 不支持路径级写入限制时，由 [09 §4](../09-guardrails/README.zh-CN.md) 在任务结束后校验实际改动范围。
- Tokage 不替用户开启「跳过全部权限确认」类参数。
- 模型在执行角色下发起的命令，再按 [11](../11-command-authorization/README.zh-CN.md) 的授权模式（每次询问 / 敏感询问 / 全自动）与指令名单处理；Adapter 通过能力位 `APPROVAL_CALLBACK` / `TOOL_RULES` / `DENIAL_EVENT` / `SHELL_WRAP` / `OS_SANDBOX` 声明可用手段。

## 7. 执行模式

### 7.1 Headless（默认）

- `tokio::process::Command`；输入经参数或一次性写入 stdin；`stdout` / `stderr` 管道按行读取（单行上限 1 MiB）。
- 每行先经输出过滤（[09 §1](../09-guardrails/README.zh-CN.md)），再交给 `parse_line`。
- 一轮一个进程；多轮通过 `resume_id` 续接，Prompt Cache 对续接同样有效。不传 `resume_id` 即新窗口。

### 7.2 PTY（兜底）

用于不支持 headless 的 CLI，或只能在交互模式执行的斜杠命令。

- `portable-pty` 固定尺寸（200×50）；以续接参数进入目标会话。
- 输出经 `vte` 解析到虚拟屏幕，按「稳定帧」（300ms 无变化）取快照。
- 以匹配完成提示、进程退出或超时为结束条件，执行完立即退出，不保留常驻 PTY。

## 8. 探测与原版会话透视

探测：

```
refresh_providers()
  for adapter in registry（并发，各 10s 超时）:
     1. which(exe)          → 未找到: NOT_INSTALLED
     2. exe --version       → 超出版本区间: UNSUPPORTED_VERSION（仍可用）
     3. 登录态（只检查）：CLI 状态命令 > 凭据文件 / 钥匙串条目是否存在（不读内容）> UNKNOWN_AUTH
     4. 模型列表：CLI 列表命令 > 定义包中该 Provider 的模型
  写入 provider_health，推送 ProviderDetected
```

`ProviderStatus`：`READY | NOT_INSTALLED | NEEDS_LOGIN | UNSUPPORTED_VERSION | UNKNOWN_AUTH | DEGRADED | MELTED`。需要登录时只提示用户运行官方登录命令，并提供「在终端中打开」。

原版会话透视（只读、零 Token）：

| Provider | 数据位置（以 Adapter 支持的版本为准） | 格式 |
|----------|-----------------------------------|------|
| Claude Code | `~/.claude/projects/<编码后的项目路径>/*.jsonl` | JSONL |
| Codex CLI | `~/.codex/sessions/**` | JSONL |
| Cursor CLI / Copilot CLI / Antigravity CLI | 待确认 | 待确认 |
| Aider | 项目根 `.aider.chat.history.md` | Markdown |

`notify` 监听目录变化增量读取，映射为 `NativeSession`。前端侧栏 `ExternalSessionsSection`（F17.a）调用 `listNativeSessions` 只读展示供应商徽章、标题与更新时间（按当前项目 cwd 过滤，30 秒刷新）。格式解析失败时降级为仅显示文件名与修改时间。

## 9. 错误分类

| 类别 | 处理 |
|------|------|
| `SpawnFailed` | 重新探测，置 NOT_INSTALLED |
| `AuthRequired` | 置 NEEDS_LOGIN，排队中的任务交由匹配引擎重新分配 |
| `RateLimited` | 交给 UsageEngine 熔断 |
| `Timeout` | 回收进程组，交给编排引擎的修复策略 |
| `ParseError` | 回退纯文本记录，用量改为本地估算，标记 Adapter 需更新 |
| `NonZeroExit` | 记录 stderr 尾部 4KB |

## 10. 测试策略

- 每个 Adapter 配套录制的输出样本，`parse_line` 做快照测试，覆盖用量、缓存、工具事件提取。
- `FakeAdapter`（`chat/fake.rs`，仅测试）：以 `/bin/sh` 脚本冒充指定 CLI，走与真实一轮相同的 `run_cli`（进程组、环境裁剪、文件追踪）。可回放样本，注入延迟、卡死、刷屏、派生孙进程、越界写入、读取凭据路径、打印环境变量、指定的成功 / 失败结果，供上层引擎测试。
- 真实 CLI：默认套件含非 `#[ignore]` 的 `chat::live::smoke_detect_skip_if_absent`（探测 + `executable`，无 CLI 则 skip 通过；`TOKAGE_LIVE_SMOKE=1` 时再跑一轮廉价 turn）。完整 turn 仍在 `#[ignore]`。入口：`scripts/run-live-cli-e2e.sh`。
