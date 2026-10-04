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
| 样式 / 组件 | Tailwind CSS 4 + shadcn/ui | |
| 前端状态 | Zustand + TanStack Query | 见 [10](../10-frontend/README.md) |
| 后端运行时 | Rust + Tokio（multi-thread） | |
| 子进程 | `tokio::process`（headless）/ `portable-pty`（兜底） | 见 [01](../01-native-environment-engine/README.md) |
| ANSI 解析 | `vte` + `strip-ansi-escapes` | |
| 代码结构解析 | `tree-sitter`（常见语言 grammar） | 生成项目摘要，见 [02](../02-planning-engine/README.md) |
| 存储 | SQLite 3 + `rusqlite`（bundled）+ `refinery` | WAL，单写线程 + 读连接池 |
| Token 估算 | `tiktoken-rs` + 按 Provider 校准系数 | |
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
    tokage-native/         # Provider 探测、Adapter、PTY
    tokage-planning/       # 分流、项目摘要、规划调用、计划解析与校验
    tokage-matching/       # 定义包、CostModel、成功概率、任务分配
    tokage-orchestration/  # DAG 调度、队列、验收关卡、修复策略
    tokage-context/        # 任务简报、窗口复用、上下文动作规划
    tokage-usage/          # 用量采集、预算、节省账本、熔断
    tokage-session/        # 会话树、事件总线
    tokage-storage/        # SQLite 访问层与迁移
    tokage-auth/           # 指令名单、授权模式、审批队列、沙箱画像、凭据隔离
    tokage-proxy/          # 本地 CONNECT 代理、系统代理读取
  src/                     # Tauri 入口、command 注册、引擎组装
```

> 现状（2026-10-04）：已拆出 `tokage-core`（`ProviderId` / `ProviderStatus`、对话事件模型、`DomainEvent` 与总线）、`tokage-native`（登录环境、探测、Adapter、进程组、git、用量读取）、`tokage-storage`（SQLite 与迁移）；Tauri command、`knowledge`、`open_in` 留在 `src-tauri/src/`。其余 crate 在所属阶段开始时拆出，不建空壳。

所有 crate 依赖 `core`；引擎之间通过组装层注入的 trait 对象与事件总线通信，便于单测替换。`CostModel` 由 `tokage-matching` 实现，供规划（选规划模型）、匹配、上下文三处共用。

## 6. 内部事件总线

```rust
pub enum DomainEvent {
    ProviderDetected { provider: ProviderId, status: ProviderStatus },
    TriageDone { main_id: i64, route: TriageRoute },                    // DIRECT | PLAN
    PlanDrafted { main_id: i64, plan_id: i64, version: u32 },
    PlanApproved { plan_id: i64 },
    TasksAssigned { plan_id: i64 },
    TaskStateChanged { task_id: i64, state: TaskState },
    AttemptStarted { task_id: i64, attempt_id: i64, provider: ProviderId, model: String },
    AttemptOutput { attempt_id: i64, chunk: OutputChunk },
    TurnFinished { attempt_id: i64, usage: TurnUsage },
    GateResult { task_id: i64, passed: bool, report_id: i64 },
    ContextActionPlanned { attempt_id: i64, action_id: i64 },
    PlanFinished { plan_id: i64, outcome: PlanOutcome },
    UsageUpdated { provider: ProviderId },
    ProviderMelted { provider: ProviderId, reason: MeltReason, until: Option<DateTime<Utc>> },
    ProviderRecovered { provider: ProviderId },
}
```

> 现状（2026-10-04）：`tokage_core::Bus` 已接入，每个订阅者按发布顺序收到全部事件，不丢弃；订阅者退出后在下次发布时移除。目前发布 `ProviderDetected`，以及 P7 建立执行窗口前的过渡事件 `ChatOutput { turn_id, event }`、`ChatEnded { turn_id }`；应用层订阅后转发为前端的 `chat-event`。其余事件随所属阶段加入。

## 7. IPC 契约（Tauri Command）

参数与返回值以 `serde` 序列化，TypeScript 类型手写于 `src/lib/ipc.ts`，与 Rust 定义逐一对应。暂不采用 `specta` / `tauri-specta`：2.0 自 2023-07 起一直处于 RC（rc.25），RC 之间有破坏性变更且须与 tauri 依赖的 specta 版本同步升级；`u64` / `i64` 字段与 `serde_json::Value` 需逐个标注或重映射；`tauri-specta` 把返回值包成 `{ status, data | error }`，所有调用点都要改。2.0 正式发布后重新评估。

> 现状（2026-10-04）：已注册的 command 为 `refresh_providers`、`refresh_provider`、`reload_shell_env`、`git_info`、`create_session_worktree`、`remove_session_worktree`、`read_project_file`、`list_open_targets`、`open_path_in`、`open_external_url`、`start_chat_turn`、`cancel_chat_turn`；对话事件经 `chat-event` 事件推送。下表为目标契约。

| Command | 用途 |
|---------|------|
| `list_providers` / `refresh_providers` | Provider 状态 |
| `add_project` / `list_projects` / `remove_project` | 项目管理 |
| `submit_request(project_id, text, opts)` | 新需求：分流 → 规划或直接分发；`opts.mode = auto / plan / direct` |
| `get_plan(plan_id)` / `update_plan(plan_id, patch)` | 读取 / 修改计划（任务、契约、分配） |
| `approve_plan(plan_id)` / `discard_plan(plan_id)` | 确认执行 / 放弃 |
| `replan(plan_id, instruction)` | 增量重新规划 |
| `preview_assignment(plan_id)` | 预览每个任务的模型分配与预估成本 |
| `reassign_task(task_id, model_id)` | 手动指定模型 |
| `pause_plan` / `resume_plan` / `cancel_task` / `retry_task` | 队列控制 |
| `send_message(attempt_id, text)` | 对某个执行窗口追加输入 |
| `apply_context_action` / `dismiss_context_action` | 上下文建议 |
| `get_session_tree(main_id)` / `get_chat_logs(attempt_id, after, limit)` | 会话树与日志 |
| `get_usage_dashboard(range)` | 用量、首次通过率、节省 |
| `check_knowledge_update` / `apply_knowledge_update` | 定义包更新 |
| `subscribe_events(channel)` / `set_focus(attempt_ids)` | 推送通道 / 可见窗口 |
| `list_pending_approvals` / `resolve_approval(id, decision)` | 指令审批 |
| `get_command_policy` / `update_command_rules` / `set_auth_mode` | 指令名单与授权模式 |
| `classify_command(request)` | 按名单判定一条命令（级别、原因、写入路径） |
| `get_proxy_settings` / `update_proxy_settings` / `test_proxy(provider)` | 网络代理 |
| `get_knowledge_pack(layer)` / `import_custom_pack` / `remove_custom_pack` | 定义包查看与自定义 |
| `get_build_info` | 版本、commit、构建类型、源码链接 |

## 8. 安全与隐私

- Tauri capability 只开放 Tokage 自身 command，不向前端开放 `shell`、`fs` 插件通配权限。
- 子进程命令只由后端 Adapter 构造，用户输入经 `Command::arg` 或 stdin 传入，不经 `sh -c`。
- 计划中的验收命令在用户审阅计划时可见，且须匹配允许列表（[09 §5](../09-guardrails/README.md)）。
- Tokage 只读取 CLI 配置中的非密钥字段用于展示，Key 值不保留（R11，[11 §7](../11-command-authorization/README.md)）。
- 模型发起的命令经授权模式与指令名单处理；执行角色的 CLI 在系统级沙箱内运行，看不到凭据路径与其他厂商的 Key（[11](../11-command-authorization/README.md)）。
- 唯一的外网请求是模型定义包更新，需签名校验，可在设置中关闭。

## 9. 跨平台

| 项 | macOS / Linux | Windows |
|----|---------------|---------|
| 登录 Shell 环境 | `$SHELL -ilc 'env -0'` 抓取一次 | 继承进程环境 + 注册表 PATH |
| 进程组回收 | `setsid` + `killpg(SIGTERM → SIGKILL)` | Job Object `KILL_ON_JOB_CLOSE` |
| 系统级沙箱 | macOS `sandbox-exec`；Linux Landlock / bubblewrap | v1 不提供 |
| 系统代理读取 | SystemConfiguration；桌面设置 / 环境变量 | 注册表 / WinHTTP |
| PTY | `portable-pty`（openpty） | ConPTY（Win10 1809+） |
| CLI 可执行文件 | `which` crate | 同时匹配 `.cmd` / `.exe` |
