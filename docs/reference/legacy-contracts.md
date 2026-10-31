# 历史契约与 IPC 统计参考

提取日期：2026-10-31。以下内容来自 cbcc7a31e4dd034c5153b37557299a348ecbaf30 的原专项清单附录 A–G，仅用于解释旧编号、接口和统计方法，不是当前接口规格或施工清单。接口以源码为准，后续工作只在 [项目 Todo](../TODO.md) 登记。原始正文和完整进度保留在 Git 历史。

## 附录 A 契约参考

本附录是各专项用到的类型、签名、事件与 SQL 的集中说明。**现有**指 `master@e291011` 的实际定义；**新增 / 变更**指本文规定的目标契约，以各专项为准。

### A.1 `ChatRequest`

Rust：`tokage-native/src/chat/mod.rs:142-203`（`#[serde(rename_all = "camelCase")]`）；TS：`src/lib/ipc.ts:766-798`。

```ts
interface ChatRequest {
  turnId: string; cwd: string; prompt: string;
  mode: "plan" | "direct";
  provider: ProviderId;                  // "CLAUDE_CODE" | "CODEX_CLI" | "CURSOR_CLI" | "COPILOT_CLI" | "ANTIGRAVITY_CLI" | "AIDER" | "OPENCODE" | …
  model: string;                         // CLI 接受的模型 id
  effort: string | null; fast: boolean; tier: string | null;
  history: { role: "user" | "assistant"; text: string }[];
  resume: string | null; fork?: boolean; // resume = 上一轮 thread 事件给出的 CLI 会话 id
  trusted: ProviderId[];                 // 允许获得工作区信任的 CLI；不在其中的 CLI 不会被启动
  planFile?: string | null; sessionId?: string | null; projectId?: string | null;
  sessionAllowed?: string[];             // 已授权的命令模式（审批续接用）
  authMode?: "ask_always" | "ask_sensitive" | "full_auto" | null;
  role?: "PLANNER" | "EXECUTOR" | "TESTER" | "REVIEWER" | null;   // 缺省：plan → PLANNER，direct → EXECUTOR
  writeScope?: string[];
  timeoutTier?: "QUICK" | "STANDARD" | "LONG" | "PLANNING" | "GATE" | "AGENT" | null;   // 变更：AGENT 取代 INFINITE（F3）
  timeoutMultiplier?: number | null;
  // —— 新增 ——
  idleTimeoutSecs?: number | null;       // F3：仅 AGENT 档位使用，缺省 1800
  hardTimeoutSecs?: number | null;       // F3：仅 AGENT 档位使用，缺省 7200
  planId?: number | null;                // F12：plans.id
  taskId?: number | null;                // F12：tasks.id
  domain?: string | null;                // F12
  phase?: "PLANNING" | "EXECUTION" | "REPAIR" | "CONTEXT" | "DIRECT" | null;   // F12：缺省按 (role, mode) 推导
  attemptId?: string | null;             // F12 / F13：usage_events 与 attempts 的 id；缺省沿用 "<session>|<provider>|<model>"
}
// Rust 还有 proxy: Option<GlobalProxyConfig>，前端不传，start_chat_turn 从 AppState 补。
```

`TimeoutTier` 档位表（`mod.rs:58-88`，idle / hard）：`QUICK` 15 s / 30 s、`STANDARD` 90 s / 180 s、`LONG` 180 s / 420 s、`PLANNING` 300 s / 900 s（09 §2.1 写的是 180 / 420，F3.g 改文档）、`GATE` 120 s / 300 s、`AGENT`（新增）idle / hard 来自请求，缺省 1800 s / 7200 s。`TOKAGE_IDLE_TIMEOUT_MS` / `TOKAGE_HARD_TIMEOUT_MS` 环境变量在测试里覆盖 idle / hard（`TimeoutLimits::resolved`）。

### A.2 事件与 `DomainEvent`

| 事件（Tauri） | 载荷 | 状态 |
|---------------|------|------|
| `chat-event` | `ChatEvent`（A.3） | 现有 |
| `chat-ended` | `{ turnId: string, cancelled: boolean }` | 新增（F2.d） |
| `provider-detected` | `{ scanId: string, provider: ProviderInfo }` | 现有 |
| `provider-melted` | `{ provider: string, reason: MeltReason, until: string \| null }` | 现有事件，载荷在 F15.c 补 `until` |
| `provider-recovered` | `{ provider: string }` | 现有 |
| `knowledge-pack-update-available` / `knowledge-pack-applied` | `StagedUpdateInfo` / 版本字符串 | 现有（D6 暂缓，见 F8） |

`DomainEvent`（`tokage-core/src/bus.rs`）现有 9 种：`ProviderDetected { provider, status }`、`ProviderMelted { provider, reason: String }`、`ChatOutput { turn_id, event: EventKind }`、`ChatEnded { turn_id, cancelled }`（F2）、`TurnFailed { provider, kind, message, melted_until }`、`TurnSucceeded { provider }`（F15.c）、`TriageDone { main_id, route }`、`TaskStateChanged { plan_id, task_id, old_state, new_state, reason }`、`KnowledgePackUpdateAvailable { pack_version, pending_path }`。总线保证：每个订阅者按发布顺序收到全部事件，不丢弃；订阅者被丢弃后在下一次发布时移除。

### A.3 `EventKind` / `ChatEvent`

Rust：`tokage-core/src/event.rs`（`#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]`）；TS：`ipc.ts:803-855`。`ChatEvent = { turnId } + EventKind`（扁平）。

| kind | 字段 | 说明 |
|------|------|------|
| `thread` | `id` | CLI 会话 id，下一轮 `resume` 用；每个 Adapter 都会发（Aider 固定为 `aider-session`） |
| `model` | `name` | CLI 报告的实际模型 |
| `reasoning` / `text` | `text` | 增量 |
| `toolStart` | `id, tool, title, detail` | `tool` ∈ `command / read / search / edit / web / agent / other` |
| `toolEnd` | `id, ok, output` | `output` 最长 16 KB（尾部） |
| `fileChange` | `toolId, path, absPath, previewPath, change, diff, additions, deletions` | `path` 在工作目录内是相对路径，目录外是绝对路径；`change` ∈ `added / modified / deleted`；`diff` 为空表示基线未知或过大 |
| `approvalRequest` | `id, toolId, command, level, reason, recoverable` | `id` = `apr_<turnId>_<toolId>` |
| `approvalResolved` | `id, decision` | |
| `done` | `inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, apiCalls, contextTokens, costUsd, premiumRequests, source` | `source` ∈ `STRUCTURED / STDOUT_REGEX / ESTIMATED`（`UsageSource` 还有 `CLI_OUTPUT`，只用于额度快照） |
| `error` | `message` | 超时消息格式 `Command timed out (idle timeout exceeded)` 或 `… (hard timeout exceeded)` |
| `warning` | `message` | 如越界编辑、凭据出现在 diff 里 |
| `quota` | `raw, resetAt, retrying` | 额度耗尽 / 429 |

终止语义（现状）：`Done` 之后 `run_cli` 才返回；`approval_requested` 且无 `Done` 时返回 `Ok(())` 而不发任何终止事件；`Err` 时由 `run_turn` 发 `error`；`ChatEnded` 只在总线上。F2 / F6 之后以 `chat-ended` 为准。

### A.4 计划与任务类型

Rust：`tokage-core/src/planning.rs`；TS：`ipc.ts:1389-1434`。**序列化命名不统一**（3.6）：

```ts
interface TaskItem {                     // Rust 无 rename_all：键是 snake_case
  id: string; domain: string; title: string; goal: string;
  depends_on: string[]; contracts: string[]; write_scope: string[]; deliverables: string[];
  acceptance: TaskAcceptance; size: "S" | "M" | "L"; difficulty: number /* 1-3，缺省 2 */;
  assigned_model?: string | null;        // 规范格式 "<ProviderId>:<cliModelArg>"（S6）
}
interface TaskAcceptance {               // camelCase
  commands: string[]; criteria: string[];
  confirmedCommands?: string[];          // 新增（F5.c）：用户在计划审阅里确认过的验收命令，精确字符串匹配；Rust: confirmed_commands，#[serde(default)]
}
interface PlanSpec { raw: string; goal: string; inScope: string[]; outOfScope: string[]; decisions: string[] }   // camelCase
interface PlanContract { id: string; title: string; content: string }                                          // camelCase
interface Plan {                         // camelCase
  version?: number; parentPlanId?: string | null; spec: PlanSpec; contracts: PlanContract[]; tasks: TaskItem[];
  notes?: string; rawMarkdown: string; isUserEdited?: boolean | null;
}
```

落盘：`persist_plan(cwd, planId, plan)` 写 `.tokage/plans/<planId>/{spec.md, tasks.yaml, contracts/*.md, plan.md}`，`tasks.yaml` 是 `serde_yaml::to_string(&plan.tasks)`（所以 `acceptance.confirmedCommands` 随之落盘）；`persist_plan_user_edited` 写 `user_edited/` 子目录；旧版本归档到 `history/<版本>.md`；`load_plan(cwd, planId, version?)`。`plan_id` 实际传的是**会话 id**。校验：`validate_plan(plan, max_tasks?, valid_domains?, allowed_commands?, auto_serialize_conflicts?) -> { isValid, issues: [{ severity: "info"|"warning"|"error", rule, taskId, message }], plan }`，`plan` 是被修复过的计划（域归一化、自动串行化加依赖边）。现有 `rule`：`max_tasks`、`empty_tasks`、`duplicate_task_id`、`missing_dependency`、`self_dependency`、`missing_contract`、`dependency_cycle`、`domain_mapping`、`acceptance_auto_fill`、`missing_acceptance`、`unverified_acceptance_command`、`parallel_write_conflict_auto_resolved`、`parallel_write_conflict`；新增（F5.c）：`unsafe_acceptance_command`、`unconfirmed_acceptance_command`、`acceptance_command_confirmed`。

### A.5 `TaskState` 与转换表

Rust：`tokage-orchestration/src/state.rs:21-117`；TS：`ipc.ts:1997-2009`。12 个状态（SCREAMING_SNAKE_CASE）：`PENDING`、`READY`、`RUNNING`、`VERIFYING`、`DONE`、`REPAIRING`、`BLOCKED`、`CANCELLED`、`SKIPPED`、`PAUSED_QUOTA`、`WAITING_UPSTREAM`、`INTERRUPTED`。终态 = `DONE` / `CANCELLED` / `SKIPPED`；活跃 = `RUNNING` / `VERIFYING` / `REPAIRING`；自身到自身的转换总是允许。

| 从 | 允许转到 |
|----|----------|
| `PENDING` | `READY`、`CANCELLED`、`SKIPPED`、`WAITING_UPSTREAM` |
| `READY` | `RUNNING`、`CANCELLED`、`SKIPPED`、`PAUSED_QUOTA`、`WAITING_UPSTREAM`、**`BLOCKED`（派发前无法解析模型，F1.f）** |
| `RUNNING` | `VERIFYING`、`REPAIRING`、`BLOCKED`、`CANCELLED`、`PAUSED_QUOTA`、`INTERRUPTED` |
| `VERIFYING` | `DONE`、`REPAIRING`、`BLOCKED`、`CANCELLED`、`INTERRUPTED` |
| `REPAIRING` | `RUNNING`、`BLOCKED`、`CANCELLED`、`PAUSED_QUOTA`、`INTERRUPTED` |
| `BLOCKED` | `PENDING`、`READY`、`CANCELLED`、`SKIPPED`，**新增 `VERIFYING`（S1）** |
| `PAUSED_QUOTA` | `READY`、`RUNNING`、`CANCELLED` |
| `WAITING_UPSTREAM` | `PENDING`、`READY`、`CANCELLED`，**新增 `SKIPPED`（S1，取消上游时 F3.d 需要）** |
| `INTERRUPTED` | `READY`、`RUNNING`、`CANCELLED` |
| `DONE` / `CANCELLED` / `SKIPPED` | 无 |

金文件 `tests/fixtures/task_state_transitions.json`（F1.d）是这张表的机器可读版本，Rust 与 TS 的实现都与它逐项比对。

### A.6 `PipelineState`（`.tokage/execution/<会话>.json`）

Rust：`tokage-planning/src/storage.rs:262-390`；TS：`ipc.ts:2013-2041`。原子写（临时文件 + rename）；`load_pipeline_state` 在 `status == RUNNING` 时把 `RUNNING` / `VERIFYING`（F7.b 加 `REPAIRING`）任务置 `INTERRUPTED` 并把流水线置 `INTERRUPTED`，随后回写。

v1（现有）：

```json
{ "sessionId": "…", "status": "RUNNING",
  "tasks": [{ "id": "T1", "title": "…", "state": "DONE", "assignedModel": "CLAUDE_CODE:sonnet", "turnId": "subagent-…", "startedAt": 0, "endedAt": 0 }],
  "taskStates": { "T1": "DONE" }, "taskLogs": { "T1": ["…"] },
  "gateResults": [{ "taskId": "T1", "command": "npm test", "passed": true, "exitCode": 0, "durationMs": 1200, "outputTail": "…" }],
  "startedAt": 0, "updatedAt": 0 }
```

v2（F7.b 新增字段，全部可缺省）：

```json
{ "schemaVersion": 2, "planDbId": 12, "baseCwd": "/abs/project",
  "integration": { "branch": "tokage/ab12cd34-integration", "root": "/abs/app-data/worktrees/…/integration" },
  "tasks": [{ "id": "T1", "…v1 字段…": "", "provider": "CLAUDE_CODE", "threadId": "…", "cwd": "/abs/…",
              "worktree": { "root": "…", "cwd": "…", "branch": "…", "baseCommit": "…" },
              "repairStep": "IN_WINDOW_REPAIR", "allowedCommands": ["…"], "approvals": [], "costUsd": 0.12 }],
  "taskItems": [ /* 完整 TaskItem（snake_case 键） */ ], "contracts": [ /* PlanContract */ ],
  "upstreamArtifacts": { "T1": { "taskId": "T1", "title": "…", "report": "…", "changedFiles": ["…"] } },
  "assignments": { "T1": { /* TaskAssignment 快照，A.12 */ } },
  "deviationSources": ["T1"] }
```

### A.7 前端流水线状态

`src/stores/execution.ts:14-28`（现有）：`ActivePipelineSession { sessionId, status, tasks, contracts?, upstreamArtifacts?, taskStates, taskLogs, taskTurnIds?, taskStartedAt?, taskEndedAt?, gateResults?, startedAt, cwd? }`，`status` ∈ `RUNNING / PAUSED / DONE / INTERRUPTED`。新增（F6 / F7 / F9 / F11 / F12 / F13）：`planDbId?`、`baseCwd?`、`assignments?`、`deviationSources?`，以及按任务的映射 `threadIds`、`taskProviders`、`taskCwds`、`taskWorktrees`、`repairSteps`、`taskAllowed`、`taskApprovals`、`taskCostUsd`、`taskWaitReasons`（后者只在内存，不持久化）。持久化走 `persistSessionImmediate`（状态跃迁与 gate 结果）与 `persistSessionDebounced`（日志，300 ms）。运行态（`activeSubAgents`：turnId → ctx）只在内存，重启后由 `handleSubAgentChatEvent` 按持久化状态重建。

### A.8 gate 相关

```ts
interface GateCommandResult { command: string; passed: boolean; exitCode?: number | null; durationMs: number; outputTail: string; errorMessage?: string | null }
interface ScopeValidationResult { passed: boolean; declaredScopes: string[]; changedFiles: string[]; allowedFiles: string[]; outOfScopeFiles: string[] }
```

- R5（目标，F5.b）：含元字符（`|`、`>`、`<`、`;`、`&&`、`||`、反引号、`$(`）→ `Denied`（不可确认）；匹配定义包 `gate_allowlist` → 允许；否则 `confirmed` → 允许；否则 `Denied`。命令按 `shell_words_split` 切成参数数组，不经 shell。
- 定义包 `gate_allowlist`（`builtin.json`，15 条，`merge.rs` 只取 base 层）：`npm|pnpm|yarn|bun run <script>`（script 允许 `[\w.@:/-]+`，覆盖 `test:miniapp` / `build:f`）+ 仅 `--flag` 参数；或无 `run` 的 `test|build|lint|typecheck|check|format:check`；`npx tsc|vitest|jest|eslint|prettier --check|playwright test`；`cargo test|build|check|clippy|fmt --check`；`[python -m] pytest`；`ruff check|mypy|pyright`；`go test|build|vet`；`mvn|./mvnw test|verify|compile|package`；`gradle|./gradlew test|build|check|assemble`；`dotnet test|build`；`swift test|build`；`[bundle exec] rspec|rake test`；`phpunit`；`make test|build|check|lint|all`；`deno test|check|lint`。
- 现有代码常量 `DEFAULT_GATE_ALLOWLIST_PATTERNS`（9 条，`gate.rs:24-34`）尾参数 `(\s+.*)?` 任意、`make <任意目标>`，F5.b 删除。
- 默认超时 300 s（`DEFAULT_GATE_TIMEOUT_SECS`），证据取输出尾部 200 行（`GATE_EVIDENCE_TAIL_LINES`）。

### A.9 修复梯级类型

Rust：`tokage-orchestration/src/repair.rs`；TS：`ipc.ts:2211-2242`。

```ts
type RepairStep = "INITIAL" | "IN_WINDOW_REPAIR" | "REASSIGNED" | "BLOCKED";          // SCREAMING_SNAKE_CASE，next() 逐级推进
interface FailureEvidence { failedCommand?: string | null; outputTail: string; outOfScopeFiles: string[]; previousChangedFiles: string[] }   // camelCase
type RepairDecision =                                                                  // 外部标签；内层字段目标是 camelCase（F13.c 修 Rust 属性）
  | { tryInWindowRepair: { attemptId: string; evidence: string } }
  | { tryReassign: { excludeModels: string[]; evidence: string } }
  | { markBlocked: { terminalResumeCommand: string; reason: string } };
```

`decide_repair_action_cmd(task, currentStep, currentAttemptId, currentModel, provider, evidence[, isProviderMelted]) -> [RepairStep, RepairDecision]`；`evidence` 的 Markdown 由 `FailureEvidence::to_markdown()` 生成（失败命令、越界文件、尾部 200 行输出）。熔断时直接跳到第 2 步（`decide_repair_action_with_melted`）。

### A.10 调度类型

现有（`scheduler.rs`）：`ConcurrencyLimits { maxGlobalConcurrent: 4, defaultProviderConcurrent: 2, providerOverrides }`（camelCase）；`ScheduledTask { item, state, assignedProvider, assignedModel, criticalPathLength, contractDeviationPaused, deviationSourceTask }`；`DispatchIneligibleReason`（F9.b 加 `rename_all_fields`）。目标 DTO（`PickDispatchInput` / `PickDispatchOutput`、`DispatchIneligibleReason` 的 TS 形状）见 F9 的「目标契约」。关键路径长度 = 自己到最远下游的节点数（叶子为 1）；排序 = 关键路径降序、id 升序。

### A.11 简报与报告

Rust：`tokage-orchestration/src/brief.rs`；TS：`ipc.ts:2063-2109`。

```ts
interface UpstreamArtifact { taskId: string; title: string; report: string; changedFiles: string[] }            // camelCase
interface TaskBriefInput { task: TaskItem; contracts: PlanContract[]; upstreamArtifacts: UpstreamArtifact[]; failureEvidence?: string | null }
interface TaskReport { taskId, title, model, attemptId, firstPass, isLocallyGenerated, completedWork,
                       contractDeviations?: string | null, tipsForDownstream, selfCheckCriteria: string[],
                       changedFilesStat, changedFilesList: string[], gateResultsSummary, costUsd?: number | null,
                       defects?: string | null /* 新增（F13.e） */ }
```

- 简报固定头部 `TASK_BRIEF_FIXED_HEADER`（`brief.rs:7-16`，「# Tokage Task Brief v1 / ## 执行规则 …」）逐字稳定，保护提示缓存（R10）；后面依次是任务卡、契约、上游产物（`### <id> <title>` + 报告 + 改动文件）、失败证据、固定的「任务报告格式」小节（`### 完成内容`、`### 偏离契约之处（无则写「无」）`、`### 给下游的提示`、`### 验收标准自查`）。
- 报告缺少「完成内容」时由本地生成（`isLocallyGenerated = true`，不追加请求）。`contractDeviations` 非空且不是「无」才算偏离（F9.e）。
- 存库形态 `TaskReportRow { id?, taskId, modelId, attemptId, firstPass, isLocallyGenerated, summaryMd, fullMd, contractDeviation?, changedFilesStat, changedFilesList, gateResults, costUsd?, createdAt? }`（`record_task_report` / `get_task_reports(taskId)`；`task_id` 是任务 code）。
- 重规划输入 `TaskExecutionReport { taskId, status: ReplanTaskStatus, completedSummary, fullReport?, modifiedFiles }`（`replan.rs:87-101`；F16.c 从 `TaskReportRow` 映射：`completedSummary = summaryMd`、`fullReport = fullMd`、`modifiedFiles = changedFilesList`）。

### A.12 分配类型与 `assigned_model` 规范

Rust：`tokage-matching/src/assignment.rs:15-60`；TS：`ipc.ts:1849-1870`。

```ts
interface TaskAssignment {                                     // camelCase
  taskId: string; provider: string; modelId: string;           // modelId 是定义包的 model_id
  displayName: string; cliModelArg: string;                    // cliModelArg 是 CLI 接受的模型参数
  pSuccess: number; estCostUsd: number; meetsThreshold: boolean;
  reuseAttemptId?: string | null; explanation: string; alternatives: AlternativeCandidate[];
}
interface PlanAllocationResult { assignments: TaskAssignment[]; totalEstCostUsd: number; allMeetThreshold: boolean; providerUtilizations: Record<string, number> }
```

- `allocate_plan_models(plan, manual_overrides?, initial_provider_utils?)`：`manual_overrides` 是 `taskId → "PROVIDER:model"`，`model` 部分可以是 `model_id` 或 `cli_model_arg`（大小写不敏感，找不到时现状会静默落到第一个候选，F14.e 改成报错）；有熔断（`provider_health.status == "MELTED"`）的 Provider 被排除。
- **规范**（S6）：写回任务的 `assigned_model` = `"<provider>:<cliModelArg>"`；完整的 `TaskAssignment` 快照单独存在流水线状态的 `assignments` 里，学习与结算用 `provider` + `modelId`，不要从 `assigned_model` 字符串反解。
- 前端 `resolveTaskProviderAndModel(spec, providers, enabled, trust, blockedModels) -> ResolvedModel | null`（`taskRunner.ts:81-212`）把 spec 映射到 CLI 目录（`ProviderInfo.modelGroups[].variants[]`）里的 `{ provider, model, effort, fast, tier }`；匹配顺序：`<Provider>:` 前缀 → 目录里按 `key` / `name` / 变体 id 匹配；Provider 别名表（`ANTHROPIC` / `CLAUDE` → `CLAUDE_CODE`、`OPENAI` / `CODEX` → `CODEX_CLI`、`GITHUB` / `COPILOT` → `COPILOT_CLI`、`GOOGLE` / `ANTIGRAVITY` → `ANTIGRAVITY_CLI` 等）。

### A.13 存储表、约束与写读方

迁移 V1–V9（`tokage-storage/migrations/`）。**现状**列指生产路径。

| 表 | 关键列 / 约束 | 写入方（现状） | 读取方（现状） |
|----|---------------|----------------|----------------|
| `plans` | `id INTEGER PK`、`main_session_id` 外键、`version`、`parent_plan_id TEXT`、`est_cost_usd`、`status CHECK` ∈ DRAFT / APPROVED / RUNNING / PAUSED / COMPLETED / SUPERSEDED / ABANDONED | 无（`insert_plan_record` 无调用方） | 仪表盘、`settle_plan_savings` |
| `plan_contracts` | `plan_id` 外键、`code`、`UNIQUE(plan_id, code)` | 无 | 无 |
| `tasks` | `id INTEGER PK`、`plan_id`、`code`、`role CHECK` ∈ EXECUTOR / TESTER / REVIEWER、`state CHECK`（**只有 9 个值：PENDING / READY / RUNNING / VERIFYING / REPAIRING / DONE / BLOCKED / CANCELLED / SKIPPED，不含 PAUSED_QUOTA / WAITING_UPSTREAM / INTERRUPTED**）、`actual_cost_usd`、`UNIQUE(plan_id, code)` | 无 | `settle_plan_savings` |
| `task_edges` | `(plan_id, from_task, to_task)` | 无 | 无 |
| `attempts` | `id TEXT PK`、`main_session_id` 外键、`task_id INTEGER`、`role CHECK` ∈ PLANNER / EXECUTOR / TESTER / REVIEWER、`exec_mode`、`workdir`、`status TEXT` | 聊天快照写入（按「会话 + provider + model」建行，`repo.rs`）；`upsert_attempt_cmd` 无调用方 | `list_attempts_cmd`（`PipelineView`、`SessionHistoryView`） |
| `usage_events` | `attempt_id` 外键（`insert_usage_event` 在行不存在时置空）、`task_id INTEGER`、`plan_id INTEGER`、`phase CHECK` ∈ PLANNING / EXECUTION / REPAIR / CONTEXT / DIRECT、`source CHECK` ∈ STRUCTURED / STDOUT_REGEX / ESTIMATED、`cost_usd`、`equiv_cost_usd` | `start_chat_turn`（`task_id` / `plan_id` / `domain` / `equiv_cost_usd` 恒空；`phase` 可能违反 CHECK） | 仪表盘 |
| `task_reports` | `task_id TEXT`（任务 code）、`attempt_id TEXT` | `record_task_report`（`taskRunner`） | `get_task_reports`（无调用方） |
| `gate_runs` | `task_id TEXT`、`plan_id TEXT`、`attempt_id TEXT` | 无（`record_gate_run` 无调用方） | `list_gate_runs`（`PipelineView`，传常量 `"plan-1"`） |
| `task_outcomes` | `result CHECK` ∈ FIRST_PASS / FIXED_IN_WINDOW / REASSIGNED / BLOCKED / USER_TAKEOVER、`p_predicted`、`est_cost_usd`、`actual_cost_usd`、`turns` | 无 | 仪表盘（首次通过率矩阵） |
| `learned_stats` | PK `(provider_name, model_id, domain, bucket, metric)`，`provider_name = '*'` 表示模型级；`metric` ∈ success_alpha / success_beta / fix_rate / turns / calls_per_turn / growth_per_turn / output_per_turn / cost_calibration | `record_task_outcome`（无调用方） | 无 |
| `savings_events` | `kind CHECK` ∈ ASSIGNMENT / CONTEXT / DIRECT、`plan_id` 外键 → `plans`、`baseline_usd`、`actual_usd`、`settled` | 无 | 仪表盘 |
| `provider_health` | PK `provider_name`、`status`、`melt_reason`、`melted_until`、`consecutive_fails`、`info_json` | provider 列表快照（覆盖 `status`）与熔断命令（无调用方） | `allocate_plan_models`（`status == "MELTED"`）、仪表盘 |
| `context_actions` | `kind CHECK` ∈ CONTINUE / COMPACT / FRESH / REUSE、`trigger`、`status` | 无 | 无 |
| `approvals` | `decision CHECK` ∈ allow_once / allow_session / allow_project / deny | `resolve_approval` | `list_approvals`（`SecuritySettings`） |
| `custom_packs` | `name`、`position`、`enabled`、`pack_json`、`warnings_json` | 快照写入（先清空再插一行，至多 1 行） | `Snapshot.custom_pack` |
| `model_overrides` | `(provider_name, model_id, patch_json)` | 无（没有 IPC） | 仪表盘的 `get_effective_pack` |
| `main_sessions` | `status CHECK` ∈ PLANNING / REVIEW / RUNNING / PAUSED / BLOCKED / IDLE / COMPLETED | 快照写入（`state_to_db` 只映射 review / running / done / blocked，其余 IDLE） | 快照读取 |

写入通道：`storage_write(batch)` 排入写线程（100 ms / 最多 200 个 job 一个事务，每个 job 独立 savepoint）；带 `.await` 的 `Storage::xxx().await` 等事务提交。违反 CHECK / 外键的 job 只让自己回滚，调用方若忽略返回值就看不到错误。

### A.14 设置项

新增 / 变更（`src/stores/settings.ts`，整体持久化为 `settings.<key>`）：

| 键 | 类型 | 默认值 | 说明 | 专项 |
|----|------|--------|------|------|
| `settingsVersion` | number | 2 | 设置迁移版本（S10） | F1.g |
| `isolation` | `"worktree" \| "inplace"` | `"worktree"`（F11.i） | — | F1.g、F11 |
| `knowledgeAutoUpdate` | boolean | `false`（原 `true`） | | F8.b |
| `agentIdleTimeoutMin` | number | 30 | 范围 1–480，步长 5 | F3.c |
| `agentHardTimeoutMin` | number | 120 | 范围 5–1440，步长 10，且 ≥ `agentIdleTimeoutMin` | F3.c |
| `gateEnvAllow` | string[] | `[]` | 精确名或 `PREFIX_*`；命中凭据规则也放行，设置页须警示 | F5.d |
| `meltThreshold` | number | 0.95（原 0.9） | QUOTA_SOFT 阈值，与 06 §6.1 一致 | F15.e |

其余现有设置的接线现状见附录 D。UI：`OtherSettings.tsx` 的 Execution 卡片（`:560-581`）与「关于」；i18n 键在 `settings.execution.*`，`src/i18n/locales/en.ts` 与 `zh.ts` 都要加（`i18next.d.ts` 约束键）。

### A.15 终端接管命令表

以各 Adapter 的 `launch` 为准（`claude.rs:73-76`、`codex.rs:17-18`、`cursor.rs:78-79`、`copilot.rs:41-42`、`antigravity.rs:40-41`、`opencode.rs:30-31`、`aider.rs:28-30`），命令名取注册表（`providers.rs:164-201`）。F10.b 的 `terminal_resume` 以此为期望值：

| Provider | 命令名 | 续接命令 | 备注 |
|----------|--------|----------|------|
| `CLAUDE_CODE` | `claude` | `claude --resume <id>` | fork 时另有 `--fork-session` |
| `CODEX_CLI` | `codex` | `codex resume <id>` | headless 是 `codex exec … resume <id>`；交互式续接未实测，F10.f 验证 |
| `CURSOR_CLI` | `cursor-agent`（别名 `agent`） | `cursor-agent --resume <id>` | |
| `COPILOT_CLI` | `copilot` | `copilot --resume <id>` | |
| `ANTIGRAVITY_CLI` | `agy` | `agy --resume <id>` | |
| `OPENCODE` | `opencode` | `opencode --session <id>` | |
| `AIDER` | `aider` | 无 | Aider 没有按 id 续接，Adapter 用 `--restore-chat-history`；`terminal_resume` 返回 `None`，UI 提示「在任务目录运行 `aider --restore-chat-history`」 |

生成的整行是 `cd '<任务 cwd>' && <命令名> <参数…>`（路径做 shell 单引号转义）。

### A.16 沙箱

`tokage-native/src/sandbox/mod.rs`。画像 `SandboxProfile { write: string[], read: string[], deny: string[], keychain: bool }`，占位符：`{worktree}`、`{project}`、`{tmp}`（含 `/tmp`、`/private/tmp` 与系统临时目录的别名）、`{tokage_app_data}`、`{other_cli_dirs}` / `{forbidden_paths}`（定义包 `command_policy.forbidden_paths` 去掉本画像自己的路径）、`~/…`。

- macOS：`sandbox-exec -p <SBPL>`；SBPL 是 `(allow default)` + `(deny file-write*)` 再白名单放行写入，`deny` 列表禁读写，`keychain: false` 时禁钥匙串（`com.apple.SecurityServer` 与 `~/Library/Keychains`），强制代理时 `(deny network-outbound)` 只放行 `localhost:<port>`。**默认不限制网络。**
- Linux：优先 bubblewrap（`--ro-bind / /`、`--bind` 放行写入目录、`--tmpfs` / `--ro-bind-try /dev/null` 遮蔽凭据；不带 `--unshare-net`，网络共享），其次 Landlock（`pre_exec` 施加，无网络规则），都没有则无沙箱运行并 `tracing::warn!`。
- 其他平台：无沙箱。
- `create_sandboxed_command(provider, exe, args, worktree, home, app_data, forced_port)`（`mod.rs:460-`）按 `provider` 在 `builtin_profiles()` 里取画像，缺失时用默认画像（写 `{worktree}` + `{tmp}`、读 `{project}`、禁 `{other_cli_dirs}` / `{forbidden_paths}` / `{tokage_app_data}/**`）。F5.f 把构造函数改成接收 `&SandboxProfile`。
- 现有沙箱测试（`sandbox/tests.rs`）：`builtin_profiles_are_loaded`、`builtin_forbidden_paths_loaded`、`is_own_path_detection`、`sbpl_interpolates_paths_and_preserves_own_access`、`sbpl_enforces_keychain_deny_by_default`、`sbpl_syntax_is_valid_on_macos`、`sbpl_syntax_with_forced_proxy_is_valid_on_macos`、`landlock_profile_generation_and_credential_exclusion`、`bwrap_args_generation_and_masking`、`turn_helpers_construct_profiles`；运行时拦截的验收用例在 `chat/fake.rs`（`sandbox_intercepts_credential_redline_reads`、`full_auto_rm_outside_workspace_is_blocked_by_sandbox`）。

## 附录 B 测试地图与辅助

### B.1 Rust：FakeAdapter 与辅助函数

`tokage-native/src/chat/fake.rs`。`FakeAdapter::new(provider, steps)` 用 `/bin/sh` 走与真实轮次相同的 `run_cli`（进程组、裁剪环境、文件追踪），可回放录制样本或故意出怪。`Step`：`Replay(样本名)`、`Emit(json)`（`{"fake": "thread"|"text"|"tool"|"toolEnd"|"file"|"done"|"fail", …}`）、`DelayMs(n)`、`Hang { pidfile }`（写 pid 后永不结束）、`Flood { lines }`、`Grandchild { pidfile }`（留下比 CLI 活得久的后台进程）、`Write { path, text }`（任意位置写文件并报告编辑）、`Read { path }`、`ProbeRead { path }`、`ProbeCommand { cmd, id }`、`PrintEnv`、`Exit(code)`、`Denial { tool_id, command, reason }`、`WritePid(path)`。

测试辅助都是 `fake.rs` 内 `mod tests` 的私有函数（`:244-340`）：`env(home, extra) -> ShellEnv`（最小 PATH / HOME）、`run(fake, env, cwd)` / `run_with_mirror` / `run_req(fake, env, req)`（返回 `Run { result, events }`，内部调用 `run_cli(&FakeAdapter::exe(), &req, launch, env, mirror, None, &emit)`）、`pid(file)`、`gone(pid)`（轮询 `kill(pid, 0)` 最多 5 秒）、`texts(events)`。`chat::testing::request(provider)` 给出最小 `ChatRequest`。F2.c 给 `run_cli` 增加取消参数后，这几处调用要同步更新。

### B.2 Rust：已有的验收用例

- `tokage-orchestration/tests/orchestration_acceptance.rs`：`test_acceptance_p7_m1_dag_scheduling_lifecycle`、`…_m2_brief_assembly_and_report_lifecycle`、`…_m3_gate_execution_and_scope_validation`、`…_m4_repair_three_step_ladder_and_defect_task`、`…_m5_and_m6_complete_closed_loop`、`test_acceptance_fake_adapter_quota_exhaustion_and_auto_reassignment`。F4 / F5 / F9 改签名时这些用例要同步。
- `tokage-matching/tests/{allocation,cost,layered,learning}_acceptance.rs`、`tokage-planning/tests/acceptance.rs`（含 `triage_samples.json` 基准）。
- 依赖真实 CLI 的 `chat::live::*` 标了 `#[ignore]`，运行方式 `cargo test --lib chat::live -- --ignored --nocapture`。
- 运行：`cd src-tauri && cargo test -p <crate> [过滤名]`；全部：`cargo test --workspace --locked`。

### B.3 前端测试

`vitest`（`npm test` = `vitest run`），默认 node 环境，没有 jsdom。惯例：`vi.spyOn(ipc, "xxx").mockResolvedValue(…)` 覆盖 IPC；`useExecution.setState({ pipelines: {} })` 重置 store；`resetSubAgentEventListener()` 清理监听。**`inTauri` 在 vitest 里为 false，`ipc.ts` 的许多封装会返回 mock**（`executeGateCommand` → 「Mock gate pass」、`startChatTurn` → 回放脚本），新增用例必须显式 spy。`taskRunner.test.ts` 里已有的 fixture：provider 列表（`CLAUDE_CODE` / `CODEX_CLI`）、`t1` / `t2` 任务、`m4` / `m5` 用例的构造方式。其他测试：`src/lib/{cachePolicy,db,knowledge,triage}.test.ts`、`src/stores/auth.test.ts`、`src/components/{dashboard,planning,workspace}/*.test.*`。运行：`npx vitest run src/lib/taskRunner.test.ts`；类型检查 `npm run typecheck`。

### B.4 取消语义的最小复现程序（E1）

独立 crate（`tokio = "=1.49.0"`，features `rt-multi-thread` / `macros` / `process` / `time` / `sync`），不依赖仓库代码：

```rust
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
async fn run_cli(pid_tx: oneshot::Sender<u32>) {               // 对应 run_cli：kill_on_drop 的子进程
    let mut child = Command::new("sleep").arg("30").kill_on_drop(true).spawn().unwrap();
    let _ = pid_tx.send(child.id().unwrap());
    let _ = child.wait().await;
}
#[tokio::main]
async fn main() {
    // A：现状——外层监听任务里嵌套 spawn 内层任务
    let (pid_tx, pid_rx) = oneshot::channel();
    let (_bus_tx, mut bus_rx) = mpsc::unbounded_channel::<()>();
    let outer = tokio::spawn(async move {
        let run_task = tokio::spawn(run_cli(pid_tx));
        while let Some(_) = bus_rx.recv().await {}
        let _ = run_task.await;
    });
    let pid_a = pid_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    outer.abort();                                              // == cancel_chat_turn 对外层任务的 abort
    tokio::time::sleep(Duration::from_millis(1000)).await;
    println!("A 嵌套 spawn：abort 后子进程存活 = {}", alive(pid_a));       // 结果：true
    let _ = std::process::Command::new("kill").arg(pid_a.to_string()).status();
    // B：初始提交——单层 spawn
    let (pid_tx, pid_rx) = oneshot::channel();
    let single = tokio::spawn(run_cli(pid_tx));
    let pid_b = pid_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    single.abort();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    println!("B 单层 spawn：abort 后子进程存活 = {}", alive(pid_b));         // 结果：false
}
```

### B.5 常用命令

| 目的 | 命令 |
|------|------|
| 前端类型检查 / 测试 | `npm run typecheck`、`npm test`、`npx vitest run <文件>` |
| Rust 全量（CI 把根 crate 拆成单独作业） | `cd src-tauri && cargo test --workspace --exclude tokage --locked`；`cargo test -p tokage --locked`（根 crate，需要 Tauri 系统依赖） |
| 单个 crate / 过滤 | `cargo test -p tokage-orchestration gate`、`cargo test -p tokage-native cancel` |
| 开发运行 | `./start_tokage_dev.sh`、`npm run tauri dev` |
| 无调用方命令统计 | 附录 F 的脚本，或 F0.c 之后的 `python3 scripts/check-ipc-wiring.py` |

## 附录 C 无调用方命令的逐项处置（48 个）

统计口径见附录 F。「处置」：**接线**＝在对应专项里接到生产路径并配一条经生产入口的集成测试；**删除**＝连同 `ipc.ts` 封装一起删除；**豁免**＝保留为库能力并写入 `scripts/ipc-wiring-allowlist.txt`（带理由）。共 36 个接线、11 个删除、1 个豁免。

| 命令 | 前端封装 | 处置 | 专项 |
|------|----------|------|------|
| `validate_write_scope_cmd` | `validateWriteScope` | 接线 | F5.i |
| `revert_out_of_scope_files_cmd` | `revertOutOfScopeFiles` | 接线 | F5.j |
| `decide_repair_action_cmd` | `decideRepairAction` | 接线 | F13.c |
| `generate_implementation_fix_task_cmd` | `generateImplementationFixTask` | 接线 | F13.e |
| `record_gate_run` | `recordGateRun` | 接线 | F13.b |
| `get_task_reports` | `getTaskReports` | 接线 | F7.c、F16.c |
| `generate_delivery_summary_cmd` | `generateDeliverySummary` | 已接线 | F11.f |
| `schedule_plan_dag` | `schedulePlanDag` | 删除（被 `pick_dispatchable_tasks` 取代） | F9.b |
| `upsert_attempt_cmd` | `upsertAttempt` | 接线 | F13.b |
| `update_main_session_status_cmd` | `updateMainSessionStatus` | 删除（会话状态走 TS `setSessionState`） | F13.b |
| `derive_session_title_from_spec_cmd` | `deriveSessionTitleFromSpec` | 接线 | F13.b |
| `check_plan_budget_overrun_cmd` | `checkPlanBudgetOverrun` | 接线 | F13.f |
| `record_task_outcome` | `recordTaskOutcome` | 接线 | F14.g |
| `list_task_outcomes` | `listTaskOutcomes` | 接线（设置页「先验 vs 实测」） | F14.g |
| `list_learned_stats` | `listLearnedStats` | 接线（同上） | F14.g |
| `settle_plan_savings_cmd` | `settlePlanSavings` | 接线 | F14.g |
| `record_savings_event_cmd` | `recordSavingsEvent` | 接线（`DIRECT` / `CONTEXT` 事件） | F16.d、F16.e |
| `list_savings_events_cmd` | `listSavingsEvents` | 删除（仪表盘已按 `get_usage_dashboard` 聚合） | F17.c |
| `record_usage_event` | `recordUsageEvent` | 删除（用量在 Rust 内部落库） | F17.c |
| `list_usage_events` | `listUsageEvents` | 删除 | F17.c |
| `record_context_action_cmd` | `recordContextAction` | 接线 | F16.e |
| `list_context_actions_cmd` | `listContextActions` | 豁免（dashboard-only；写入已由 record/settle 接线） | F17.e |
| `settle_context_savings_cmd` | `settleContextSavings` | 接线 | F16.e |
| `trip_provider_melt_cmd` | `tripProviderMelt` | 接线（F15 软熔断；保留） | F15.e |
| `record_provider_failure_cmd` | `recordProviderFailure` | 删除（连续失败由 Rust 订阅者计入） | F17.c |
| `reset_provider_failures_cmd` | `resetProviderFailures` | 删除（同上） | F17.c |
| `recover_provider_health_cmd` | `recoverProviderHealth` | 接线（CLI 面板「手动恢复」） | F15.e |
| `list_provider_health_cmd` | `listProviderHealth` | 接线 | F15.e |
| `get_provider_utilization_now_cmd` | `getProviderUtilizationNow` | 接线 | F15.e |
| `reassign_unstarted_tasks_cmd` | `reassignUnstartedTasks` | 接线 | F15.f |
| `query_provider_quota` | （无封装） | 接线（补封装） | F15.e |
| `get_provider_proxy_status` | `getProviderProxyStatus` | 删除（`ProxySettings` 用 `getAllProviderProxyStatuses`） | F17.c |
| `get_repo_digest` | `getRepoDigest` | 接线 | F16.b |
| `render_planning_prompt` | `renderPlanningPrompt` | 接线 | F16.b |
| `assemble_replan_prompt` | `assembleReplanPrompt` | 接线 | F16.c |
| `compute_plan_diff` | `computePlanDiff` | 接线 | F16.c |
| `apply_plan_diff` | `applyPlanDiff` | 接线 | F16.c |
| `generate_adaptation_tasks` | `generateAdaptationTasks` | 接线 | F16.c |
| `assign_direct_task` | `assignDirectTask` | 接线 | F16.d |
| `apply_context_action_cmd` | `applyContextAction` | 接线 | F16.e |
| `dismiss_context_action_cmd` | `dismissContextAction` | 接线 | F16.e |
| `build_compact_cmd` | `buildCompact` | 接线 | F16.e |
| `triage_main_session_message_cmd` | `triageMainSessionMessage` | 接线 | F16.e |
| `evaluate_window_reuse_cmd` | `evaluateWindowReuse` | 接线（`findReusableWindow` / `dispatchSubAgentTask`） | F16.e+ |
| `validate_task_brief_header_cmd` | `validateTaskBriefHeader` | 接线（派发前 R10） | discipline |
| `validate_window_continuity_cmd` | `validateWindowContinuity` | 接线（resume R9） | discipline |
| `validate_user_message_integrity_cmd` | `validateUserMessageIntegrity` | 接线（follow-up R10） | discipline |
| `list_native_sessions` | `listNativeSessions` | 接线 | F17.a |
| `cleanup_retention_records_cmd` | `cleanupRetentionRecords` | 接线 | F17.b |
| `classify_command` | `classifyCommand` | 删除（设置页用 `authorizeCommand`） | F17.c |
| `get_builtin_knowledge_pack` | `getBuiltinKnowledgePack` | 删除（前端直接 `import builtin.json?raw`） | F17.c |

新增命令（接线后会出现，需要有调用方）：`reassign_task`（F13.d）、`plan_cost_summary`（F13.f）、`upsert_model_override` / `delete_model_override` / `list_model_overrides`（F14.f）、`render_plan_repair_prompt`（F16.b）。（F9–F12 相关命令已接线）

## 附录 D 设置项接线现状

对 `src/` 下除 `settings.ts` 与测试以外的文件做引用统计（2026-10-08）。「仅设置页」指只被设置页 UI 读写，没有任何业务代码消费。

| 键 | 默认值 | 现状消费者 | 处置 |
|----|--------|------------|------|
| `requestMode` / `rememberMode` / `lastMode` | `auto` / `false` / `auto` | `Workspace.tsx`、设置页 | 已接线 |
| `plannerModel` | `"auto"` | 仅设置页 | F16.f |
| `referenceModel` | `"auto"` | 仪表盘显示、设置页；结算命令的 `user_override` 没传 | F14.g（结算时传入非 `auto` 的值） |
| `confidenceThreshold` | 0.8 | 仅设置页 | F14.d |
| `requireReview` | `true` | 仅设置页 | F16.f |
| `planningAutoApproveBelowUsd` | 0 | 仅设置页 | F16.f |
| `contextMode` | `"suggest"` | `Workspace.tsx`、设置页 | 已接线 |
| `cacheAware` | `true` | 仅设置页 | F16.e |
| `contextMinSaving` | 0.15 | 仅设置页 | F16.e |
| `planBudgetUsd` | 0 | 仅设置页 | F13.f |
| `pauseOnOverrun` | `true` | 仅设置页 | F13.f |
| `meltThreshold` | 0.95 | QUOTA_SOFT 轮询阈值 | F15.e |
| `activeQuotaQuery` | `false` | 仅设置页 | F15.e |
| `timeoutMultiplier` | 1 | `stores/chat.ts`、`taskRunner.ts`（传入 `ChatRequest`） | 已接线；`AGENT` 档位不乘它（F3） |
| `isolation` | `"worktree"`（设计默认；F1.g 曾暂改 `inplace`） | `taskRunner.ts`、设置页 | F1.g、F11 |
| `maxParallel` | 3 | `taskRunner.ts`、设置页 | F9（映射为 `maxGlobalConcurrent`） |
| `sessionWorktree` | `false` | `Workspace.tsx` | 已接线 |
| `knowledgeAutoUpdate` | `true`（改为 `false`） | `KnowledgeSettings.tsx` | F8.b |
| `providerEnabled` / `providerPaths` / `providerAdded` / `providerTrust` | 空 | CLI 管理、模型目录、`taskRunner.ts` | 已接线 |
| `authMode` / `authProjectModes` | `ask_sensitive` / 空 | 安全设置、`stores/auth.ts`、`stores/chat.ts` | 已接线 |
| `tokageGitignore`、`welcomeSeen` | 空 / `false` | `stores/chat.ts`、`App.tsx` | 已接线 |
| `blockedModels` / `modelBanningLevel` / `modelProviderPreferences` | 空 / `off` / `{opus: …}` | `Workspace.tsx`、`TasksTable.tsx`、`PipelineView.tsx`、`models.ts`、`taskRunner.ts` | 已接线 |
| `devShadowModel` | `"none"` | `usePlanSession.ts`、`stores/chat.ts`、设置页 | 开发用：把真实请求换成替身模型而界面显示不变，默认关闭；不在本文范围 |
| `settingsVersion`、`agentIdleTimeoutMin`、`agentHardTimeoutMin`、`gateEnvAllow` | 见 A.14 | （新增） | F1.g、F3.c、F5.d |

## 附录 E 红线 R1–R18

出自 09 §7；「现状」列是本文的审阅结论（F18.d 同步到文档）。

| # | 红线 | 执行位置 | 现状 |
|---|------|----------|------|
| R1 | 含 `\r` 覆盖、ANSI、spinner、进度条的原始文本不落库 | OutputFilter | 对话路径成立；gate 输出全量缓冲（F5.g） |
| R2 | 每个子进程都在 supervise 下运行，带空闲与硬超时 | NativeEngine | 子代理 `INFINITE` 与审批挂起时整段跳过（F3） |
| R3 | 超时 / 取消回收整个进程组 | NativeEngine | 取消没生效（F2）；gate 超时只杀直接子进程（F5.e） |
| R4 | 不使用 shell 字符串拼接构造命令 | ProviderAdapter / Gate | 成立 |
| R5 | 验收命令须在允许列表内或经用户确认 | OrchestrationEngine | 被 `confirmed = true` 与代码常量绕过（F5.b / F5.c） |
| R6 | 并行任务写入范围不重叠；任务结束必做范围校验 | OrchestrationEngine | 函数存在但未接线，glob 语义有误（F4 / F5.i） |
| R7 | 规划与评审窗口只读 | NativeEngine | 成立（`run_cli` 里 `Ev::File` 终止） |
| R8 | 不替用户开启 CLI 的跳过权限确认参数 | ProviderAdapter | 成立 |
| R9 | 同一窗口内不切换模型、不改变权限与启动参数 | ContextEngine | 库层成立 |
| R10 | 上下文动作只在轮次边界发生；不为保温发送额外请求 | ContextEngine | 库层成立 |
| R11 | Tokage 只读取 CLI 配置的非密钥字段；Key / Token 的值读到即丢弃；不对模型厂商发起鉴权 / 额度查询 | NativeEngine / 前端 | 成立；「不对外发请求」指不对各模型厂商发请求，定义包下载 / 检查更新属于基础设施（F8 收口官方通道） |
| R12 | 超出预算红线时暂停队列 | OrchestrationEngine | 命令存在但未接线，缺修复成本 30% 判定（F13.f） |
| R13 | 模型发起的命令按授权模式处理；`forbidden` 级命令在任何模式下拒绝 | AuthEngine | 成立 |
| R14 | 执行角色的 CLI 进程树在系统级沙箱内运行 | NativeEngine | CLI 窗口成立；gate 不在沙箱里（F5.f） |
| R15 | 模型不可读取凭据路径与其他 CLI 的配置目录 | 沙箱 / shell 包装 / 工具规则 | CLI 窗口成立；gate 不成立（F5.f） |
| R16 | 子进程环境按厂商裁剪；模型执行的 shell 中不出现凭据变量 | NativeEngine | CLI 窗口成立；gate 传完整登录环境（F5.d） |
| R17 | 日志与界面中的密钥值脱敏；Key 写入项目文件时告警 | OutputFilter | 对话路径成立；gate 输出未脱敏（F5.g） |
| R18 | 本地代理只做 CONNECT 转发，不解密、不记录内容 | ProxyRelay | 成立 |

## 附录 F 无调用方命令的统计口径

在仓库根目录运行（只读，不修改任何文件）。命令都定义在 `src-tauri/src/lib.rs`；封装名取 `src/lib/ipc.ts` 中包含 `invoke("<command>")` 的最近一个 `export function` / `export const`；「调用方」指 `src/` 下除 `ipc.ts` 和测试文件以外出现过该封装名。基线输出为 `commands=104 no_production_caller=48`。`scripts/check-ipc-wiring.py`（F0.c）在此基础上读取豁免表并设置退出码。

```bash
python3 -I - <<'EOF'
import re, pathlib
root = pathlib.Path('.')
rust = (root/'src-tauri/src/lib.rs').read_text()
cmds = re.findall(r'#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)', rust)
ipc = (root/'src/lib/ipc.ts').read_text().splitlines()
decl = re.compile(r'^export\s+(?:async\s+)?(?:function\s+([A-Za-z0-9_]+)|const\s+([A-Za-z0-9_]+)\s*=)')
cur, wrapper = None, {}
for ln in ipc:
    m = decl.match(ln)
    if m:
        cur = m.group(1) or m.group(2)
    for c in re.findall(r'invoke[^(]*\(\s*[\'"]([a-z0-9_]+)[\'"]', ln):
        wrapper.setdefault(c, cur)
prod = [p.read_text() for p in (root/'src').rglob('*')
        if p.suffix in ('.ts', '.tsx') and p.name != 'ipc.ts' and '.test.' not in p.name]
used = lambda n: any(re.search(r'\b' + re.escape(n) + r'\b', t) for t in prod)
missing = [c for c in cmds if c not in wrapper or not used(wrapper[c])]
print(f"commands={len(cmds)} no_production_caller={len(missing)}")
print(*missing, sep='\n')
EOF
```

## 附录 G 未覆盖范围

以下区域审阅时没有细读，不代表没有问题，建议在对应专项推进时顺带补审：各 CLI Adapter 的流解析细节（`chat/{claude,codex,cursor,copilot,antigravity,aider,opencode}.rs`）、沙箱画像的实际有效性（SBPL / Landlock / bwrap 在真实 CLI 下是否缺权限）、代理中继与系统代理读取（`tokage-native/src/proxy/`）、`tokage-planning` 的 parser / replan / digest、`tokage-context` 的全部实现、`tokage-matching` 的 merge / pack、`usage.rs`（额度推算）、`session.rs`（原生会话）、`providers.rs`（探测）、`src-tauri/capabilities` 与 CSP 的实际效果、绝大多数 UI 组件与 i18n、`devShadowModel` 这个开发用的模型替身开关。
