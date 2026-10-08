# 00 · 总体架构

返回 [README](../../README.md)

## 1. 目标与非目标

目标：

- 单一桌面入口，统一调度本机已安装的官方 AI CLI。
- 先规划、后分发：强力模型产出精简设计稿与任务图，各任务交给最合适的模型执行。
- 任务按依赖排队、并行执行，产物沿依赖链传递，统一受契约约束。
- 以缓存感知的方式管理每个执行窗口的上下文。
- 统一呈现订阅额度、按量账单、首次通过率与节省金额，并据此熔断与再分配。

非目标（v1）：

- 不解密、不读取、不改写厂商 API 请求，不做 HTTP 中间人；可选的本地代理只做 CONNECT 转发（[12](../12-network-proxy/README.md)）。
- 不按厂商拦截 CLI 的出网目标。
- 不托管用户 API Key；凭据由各 CLI 自己管理。
- 不做多机同步与团队协作。
- 不自研 Agent 能力；读写文件、执行工具由各 CLI 完成。

## 2. 技术栈

| 层 | 选型 | 说明 |
|----|------|------|
| 桌面壳 | Tauri 2.x | 体积与常驻内存小；capability 机制最小化前端权限 |
| 前端 | React 19 + TypeScript + Vite | |
| 样式 / 组件 | Tailwind CSS 4 + 自研 `src/components/ui.tsx` | 未采用 shadcn/ui；换用列入后续计划 |
| 前端状态 | Zustand | 未采用 TanStack Query；见 [10](../10-frontend/README.md) |
| 后端运行时 | Rust + Tokio（multi-thread） | |
| 子进程 | `tokio::process`（headless）/ `portable-pty`（兜底） | 见 [01](../01-native-environment-engine/README.md) |
| ANSI 解析 | `vte` + `strip-ansi-escapes` | |
| 代码结构解析 | 每文件前 300 行的行前缀符号提取 | 未采用 tree-sitter；生成项目摘要见 [02](../02-planning-engine/README.md) |
| 存储 | SQLite 3 + `rusqlite`（bundled）+ `refinery` | WAL，单写线程 + 读连接池 |
| Token 估算 | 字节数 ÷ 4（启发式） | 未采用 tiktoken-rs；换用列入后续计划 |
| 日志 | `tracing` + `tracing-appender` | |

## 3. 核心闭环

```
            ┌────────────────────────────────────────────────────────┐
            │                      用户需求                           │
            └───────────────────────────┬────────────────────────────┘
                                        ▼
                         ┌──────────── 分流 (本地) ────────────┐
                         │ 简单 → 直接分发单个任务               │
                         │ 复杂 → 进入规划                       │
                         └──────┬──────────────────────┬────────┘
                                ▼                      │
                 PlanningEngine：强力模型 + 项目摘要       │
                 → 设计稿 + 契约 + 任务图（精简）           │
                                │                      │
                          用户审阅 / 修改                 │
                                ▼                      ▼
                 MatchingEngine：逐任务选模型（成功概率 × 成本 × 负载）
                                ▼
                 OrchestrationEngine：DAG 队列 → 执行 → 报告 → 下游
                                │          ▲
                                ▼          │ 简报 / 窗口复用
                 ContextEngine ────────────┘
                                ▼
                 验收关卡（本地命令）→ 通过 / 修复兜底
                                ▼
                 UsageEngine：真实用量 → 学习首次通过率与用量画像 → 下次匹配更准
```

## 4. 进程模型

```
┌──────────────────────── Tokage 进程 ─────────────────────────┐
│  WebView (React)                                              │
│     │  invoke() / Channel<UiEvent>                            │
│  ───┼──────────────────────────────────────────────────────── │
│  Tauri Core (Rust, Tokio)                                     │
│     ├─ Orchestrator 组装层                                    │
│     ├─ PlanningEngine     ├─ MatchingEngine（含 CostModel）    │
│     ├─ OrchestrationEngine├─ ContextEngine                    │
│     ├─ NativeEngine ── ProviderAdapter × N                    │
│     ├─ UsageEngine        ├─ SessionEngine（事件 → 前端）      │
│     ├─ AuthEngine（授权 / 沙箱 / 凭据隔离）├─ ProxyRelay（可选） │
│     └─ Storage                                                │
└──────────────┬──────────────────────────────────────────────┘
               │ spawn（每个子进程独立进程组）
     ┌─────────┼──────────┬───────────┐
     ▼         ▼          ▼           ▼
  claude     codex      cursor-agent  agy ...  ← 官方原版 CLI
```

- 子进程工作目录为项目根（或任务专属 worktree，见 [03 §6](../03-orchestration-engine/README.md)）。
- 子进程继承用户登录 Shell 的环境变量，不额外注入业务变量。
- 每个子进程运行在独立进程组，超时或取消时整组回收（[09](../09-guardrails/README.md)）。

## 5. Rust Crate 划分

```
src-tauri/
  crates/
    tokage-core/           # 领域模型、错误、事件定义（无 IO）
    tokage-native/         # Provider 探测、Adapter、PTY、用量读取、代理探测
    tokage-planning/       # 分流、项目摘要、规划调用、计划解析与校验
    tokage-matching/       # 定义包、CostModel、成功概率、任务分配、学习
    tokage-orchestration/  # DAG 调度、队列、验收关卡、修复策略、会话树类型
    tokage-context/        # 任务简报、窗口复用、上下文动作规划
    tokage-storage/        # SQLite 访问层与迁移（含节省账本、熔断状态）
  src/                     # Tauri 入口、command 注册、policy / knowledge / dashboard
```

> 现状（2026-10-08）：实际为上述 **7 个 crate**。未创建 `tokage-usage` / `tokage-session` / `tokage-auth` / `tokage-proxy`——用量与熔断落在 `tokage-native` + `tokage-storage` + 应用层订阅者；会话树类型在 `tokage-orchestration::tree`，统一审批与最小 `UiEvent` 批处理已在前端接线（完整 `set_focus` / 重连未贯通）；指令授权与沙箱在 `tokage-core` + `tokage-native::sandbox` + `src/policy.rs`；代理在 `tokage-native::proxy` + 应用层 command。

所有 crate 依赖 `core`；引擎之间通过组装层注入的 trait 对象与事件总线通信，便于单测替换。`CostModel` 由 `tokage-matching` 实现，供规划（选规划模型）、匹配、上下文三处共用。

## 6. 内部事件总线

```rust
// tokage_core::DomainEvent — 以代码为准（2026-10-08）
pub enum DomainEvent {
    ProviderDetected { provider: ProviderId, status: ProviderStatus },
    ProviderMelted { provider: ProviderId, reason: String },
    ChatOutput { turn_id: String, event: EventKind },
    ChatEnded { turn_id: String, cancelled: bool },          // F2：取消时 cancelled == true
    TurnFailed {                                             // F15
        provider: ProviderId,
        kind: TurnErrorKind,
        message: String,
        melted_until: Option<String>,
    },
    TurnSucceeded { provider: ProviderId },                  // F15
    TriageDone { main_id: Option<i64>, route: TriageRoute },
    TaskStateChanged {
        plan_id: String,
        task_id: String,
        old_state: String,
        new_state: String,
        reason: Option<String>,
    },
    KnowledgePackUpdateAvailable { pack_version: String, pending_path: String },
}
```

> 现状（2026-10-08，F18）：`tokage_core::Bus` 保证每个订阅者按发布顺序收到全部事件；订阅者丢弃后在下次发布时移除。应用层转发：`ChatOutput` → `chat-event`；`ChatEnded` → `chat-ended`（`{ turnId, cancelled }`）；`TurnFailed` / `ProviderMelted` / 恢复 → `provider-melted` / `provider-recovered`（F15）。上表曾列的 `PlanDrafted` / `AttemptStarted` 等设计事件**尚未**作为 `DomainEvent` 变体实现。

## 7. IPC 契约（Tauri Command）

参数与返回值以 `serde` 序列化，TypeScript 类型手写于 `src/lib/ipc.ts`，与 Rust 定义逐一对应。暂不采用 `specta` / `tauri-specta`：2.0 自 2023-07 起一直处于 RC（rc.25），RC 之间有破坏性变更且须与 tauri 依赖的 specta 版本同步升级；`u64` / `i64` 字段与 `serde_json::Value` 需逐个标注或重映射；`tauri-specta` 把返回值包成 `{ status, data | error }`，所有调用点都要改。2.0 正式发布后重新评估。

> 现状（2026-10-08，F18）：已注册 command 以 `src-tauri/src/lib.rs` 的 `invoke_handler` 与 `src/lib/ipc.ts` 为准（约 119 个；`scripts/check-ipc-wiring.py` 校验生产调用方）。下表为**代表性**生产命令；完整列表见代码。F17.c 已删除无调用方命令（如 `classify_command`、`get_builtin_knowledge_pack`、`list_usage_events`、`list_savings_events_cmd`、`get_provider_proxy_status`、`record_provider_failure_cmd`、`reset_provider_failures_cmd` 等）。`evaluate_window_reuse_cmd` 豁免暂缓。

| Command | 用途 |
|---------|------|
| `refresh_providers` / `refresh_provider` / `reload_shell_env` | Provider 探测与环境 |
| `start_chat_turn` / `cancel_chat_turn` | 对话轮次（事件经 `chat-event` / `chat-ended`） |
| `triage_request` / `render_planning_prompt` / `get_repo_digest` / `validate_plan` / `persist_plan` / `load_plan` | 规划 |
| `assemble_replan_prompt` / `compute_plan_diff` / `apply_plan_diff` / `generate_adaptation_tasks` | 增量重规划 |
| `allocate_plan_models` / `assign_direct_task` / `reassign_task` | 模型分配 |
| `pick_dispatchable_tasks` / `execute_gate_command` / `cancel_gate_command` | 调度与验收 |
| `decide_repair_action_cmd` / `generate_implementation_fix_task_cmd` | 修复梯级 |
| `plan_in_window_context_cmd` / `apply_context_action_cmd` / `build_compact_cmd` / `triage_main_session_message_cmd` | 上下文 |
| `trip_provider_melt_cmd` / `recover_provider_health_cmd` / `list_provider_health_cmd` | 熔断 |
| `get_usage_dashboard` / `settle_plan_savings_cmd` / `record_savings_event_cmd` | 用量与节省 |
| `list_native_sessions` / `cleanup_retention_records_cmd` | 外部会话 / 保留清理 |
| `authorize_command` / `resolve_approval` / `get_command_policy` | 指令授权 |
| `get_all_provider_proxy_statuses` / `test_proxy_connection` | 网络代理 |
| `build_info` | 版本与构建指纹 |

## 8. 安全与隐私

- Tauri capability 只开放 Tokage 自身 command，不向前端开放 `shell`、`fs` 插件通配权限。
- 子进程命令只由后端 Adapter 构造，用户输入经 `Command::arg` 或 stdin 传入，不经 `sh -c`。
- 计划中的验收命令在用户审阅计划时可见，且须匹配允许列表（[09 §5](../09-guardrails/README.md)）。
- Tokage 只读取 CLI 配置中的非密钥字段用于展示，Key 值不保留（R11，[11 §7](../11-command-authorization/README.md)）。
- 模型发起的命令经授权模式与指令名单处理；执行角色的 CLI 在系统级沙箱内运行，看不到凭据路径与其他厂商的 Key（[11](../11-command-authorization/README.md)）。
- 不对各模型厂商发起鉴权 / 额度类请求；定义包下载与检查更新属于基础设施（签名校验，可关闭），不在此禁令内。

## 9. 跨平台

| 项 | macOS / Linux | Windows |
|----|---------------|---------|
| 登录 Shell 环境 | `$SHELL -ilc 'env -0'` 抓取一次 | 继承进程环境 + 注册表 PATH |
| 进程组回收 | `setsid` + `killpg(SIGTERM → SIGKILL)` | Job Object `KILL_ON_JOB_CLOSE` |
| 系统级沙箱 | macOS `sandbox-exec`；Linux Landlock / bubblewrap | v1 不提供 |
| 系统代理读取 | SystemConfiguration；桌面设置 / 环境变量 | 注册表 / WinHTTP |
| PTY | `portable-pty`（openpty） | ConPTY（Win10 1809+） |
| CLI 可执行文件 | `which` crate | 同时匹配 `.cmd` / `.exe` |
