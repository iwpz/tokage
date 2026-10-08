# Tokage 专项修复清单

返回 [README](../README.md) · [TODO](TODO.md) · 创建于 2026-10-08 · 决策定稿于 2026-10-08

本文把 2026-10-08 对 `master@e291011` 的代码审阅结果整理成一份**唯一权威的整改方案**：每个问题的证据、要改成什么样（含接口契约）、怎么改、怎么验收，都写在这里，接手的人读完就能开工。目标有两个：让 [TODO.md](TODO.md) 里标为完成的主链路在生产路径上真正跑通；让文档、测试与实现三者一致。本文不引入新功能。

## 0. 阅读指南与使用约定

**怎么读**

- 接手时先读第 1 节（已定稿决策）和第 2 节（修复顺序），再通读第 3 节（背景与调查结论），然后从 F0 开始按顺序做。
- 第 4 节是各专项的整改步骤。用到的类型、IPC 签名、事件、JSON / SQL 契约集中在附录 A；测试辅助与运行方式在附录 B；48 个无调用方命令的处置在附录 C；设置项接线现状在附录 D。
- **F 编号就是实际执行顺序，也是优先级**：一个专项做完再开始下一个。依赖已按此排好，没有指向后面专项的前置依赖。
- 本文是唯一权威：与代码或其他文档冲突时以本文为准，并在对应专项的「文档同步」条目里把文档改掉。所有决策已定稿，执行时不需要再做选择；唯一暂缓的是 D6（签名与发布渠道），其过渡方案和暂缓项都在 F8。

**使用约定**

- 条目写作 `F2.c`（专项 F2 的第 c 项）。完成后把 `- [ ]` 改为 `- [x]`；专项内全部条目完成且「验收」通过后，在专项标题末尾加 ✓。
- Bug 类条目先写复现用例（应先红），再修复，直到用例转绿；复现用例留在代码库里作为回归测试。条目里写明「复现用例」的，必须先于对应的修复条目提交。
- 每个子条目一个独立 commit，提交说明引用本文编号，例如 `fix(F2.c): 取消时回收真正持有子进程的任务`。
- 完成的定义：一项引擎能力要在 [TODO.md](TODO.md) 里标 ✓，必须同时满足 ① 库层有单测；② 生产路径上有调用方（`scripts/check-ipc-wiring.py` 不再报告，见 F0.c）；③ 有一条经由生产入口的集成测试。
- 每个专项的最后一项是「文档同步」：随专项把受影响的设计文档改掉，F18 只处理总览性文档。
- 验证状态：【已验证】实际运行过最小复现，或原样抽取源码函数运行；【已对照源码】对照了第三方库源码或仓库文件；【已读源码】读过相关实现与迁移但未运行；【静态推演】只读代码得出，开工前必须先用复现用例确认。
- 级别：P0 主链路不可用或进程失控；P1 安全边界或数据一致性；P2 完整度与文档。
- 代码位置写作 `path:line`，行号是 `master@e291011` 的位置，会漂移，以符号名为准。
- 任何会联网下载依赖的操作（`npm ci`、`cargo fetch` 等）都需要先得到使用者同意。

## 1. 已定稿的决策

以下决策已经做出，执行时直接照做。

- **D1 调度权归属 = A**：状态源仍在前端 store；新增 IPC `pick_dispatchable_tasks`，由 Rust `DagScheduler` 依据传入的任务定义、当前状态、分配与并发上限，计算「可派发、可晋升、被挡住及原因」；废弃 `schedule_plan_dag`。占用口径：全局并发 = `RUNNING` + `VERIFYING` + `REPAIRING`；单 Provider 并发 = `RUNNING` + `REPAIRING`；写入范围占用 = `RUNNING` + `VERIFYING` + `REPAIRING`。Provider 并发上限取定义包 `providers.<id>.concurrency_hint`（缺省 2），全局上限取设置 `maxParallel`。契约见附录 A.10，落在 F9。
- **D2 隔离策略 = 先止血，再做完整 worktree**：阶段一（F1.g）把默认隔离改回 `inplace` 并做一次设置迁移（03 §6 称之为 `shared`，代码取值是 `inplace`，本文统一用 `inplace`）；阶段二（F11）实现完整的 worktree 模式。F11 验收通过之前，`worktree` 在设置页标为「实验性」，默认值保持 `inplace`；通过后把默认值改为 `worktree`（切换条件见 F11.i）。
- **D3 gate 沙箱 = 允许联网**：gate 在系统沙箱里运行，只限制读写与凭据，不限制出站网络；强制代理模式（docs/12）只约束 CLI 窗口，不约束 gate。无系统沙箱的平台（Windows v1）降级为「环境裁剪 + 进程组回收」并在界面提示，不提供 R14 / R15 保证。gate 的环境变量 = 登录环境 − 全部凭据变量；设置项 `gateEnvAllow`（默认空）可按名称追加（语法同 `child_env`：精确名或 `PREFIX_*`），追加的名字即使命中凭据规则也放行，但设置页必须警示「这些变量会暴露给模型写的测试 / 构建脚本」。落在 F5。
- **D4 write_scope 语义 = A**：无通配符的条目 = 文件或目录前缀，规范化为 `entry/**`；含通配符的按 glob；`classify_edit` 同步使用同一套函数（行为变化：目录写法不再被整体判为越界）。落在 F4。
- **D5 子代理超时**：取消 `INFINITE` 的「完全不监督」，改为两个设置项——`agentIdleTimeoutMin`（无输出多久视为卡死，默认 30 分钟）与 `agentHardTimeoutMin`（单轮总时长上限，默认 120 分钟）。超出即终止进程组，任务置 `BLOCKED`（原因写明超时类型）。硬上限任何情况下都生效（含等待审批期间），空闲计时在等待审批期间暂停；两者都是绝对值，不乘 `timeoutMultiplier`；约束 `agentHardTimeoutMin ≥ agentIdleTimeoutMin`。落在 F3。
- **D6 签名与发布渠道 = 暂缓**：将来用 GitHub Actions 走 GitHub Release，但不是现在。现阶段只做过渡：官方更新入口在「未配置官方公钥」时整体关闭（不发网络请求），移除示例公钥常量；暂缓项只记录在 F8，不在当前验收范围内。
- **D7 无调用方命令的处置**：先接线 A、B、C 三组（执行质量闭环 F13 → 学习与节省 F14 → Provider 健康与熔断 F15），再按本文顺序做规划与上下文（F16）、杂项（F17）；每个命令的处置见附录 C。不接线的命令一律「删除，或写入豁免表并说明理由」。

**本文补充规定**（为消除歧义在上述决策之外补的，评审时可以挑战，但执行以此为准）：

| 编号 | 规定 | 落点 |
|------|------|------|
| S1 | 任务状态机新增两条转换：`BLOCKED → VERIFYING`（终端接管后重跑关卡，03 §8 要求「完成后跑验收」）与 `WAITING_UPSTREAM → SKIPPED`（取消上游时下游需要）；前端 `updateTaskState` 按同一张转换表校验，非法转换拒绝并记日志 | F1.d |
| S2 | `CANCELLED` 是终态；用户取消任务时，其传递下游中尚未开始的任务置 `SKIPPED` | F3.d |
| S3 | 验收命令的「用户确认」存放在 `TaskAcceptance.confirmed_commands`（精确字符串匹配，命令一改即失效）；allowlist 来源是定义包 `gate_allowlist`，代码里的 `DEFAULT_GATE_ALLOWLIST_PATTERNS` 删除 | F5.b、F5.c |
| S4 | 子代理触发审批后：任务置 `BLOCKED`（原因「等待审批」），用户决定后经 `READY → RUNNING` 以 CLI 会话 `resume` 续接；等待期间不占并发槽位 | F6.c |
| S5 | 应用重启后 `RUNNING` / `VERIFYING` 的任务置 `INTERRUPTED`，由用户点「继续」恢复（有 thread id 则 `resume`，否则新窗口重跑）；不自动重新派发 | F7.d |
| S6 | `assigned_model` 的规范格式为 `<ProviderId>:<cliModelArg>`；确认计划时把分配结果写回任务，并把完整的 `TaskAssignment` 快照存入流水线状态供学习与结算使用；无法解析的分配 → 任务 `BLOCKED` 并写明原因，不再静默回退到「第一个可用模型」 | F1.f |
| S7 | `learned_stats` 的 `success_alpha` / `success_beta` 改存「观测权重累计」（从 0 起），后验在分配时用 `calculate_posterior_p(prior_p0, κ, α_obs, β_obs)` 计算 | F14.b |
| S8 | `inplace` 模式下并行任务共用目录：R6 判定时排除「与本任务运行时间重叠的其他任务的 write_scope」覆盖的文件（无法归因，列为已知精度限制；需要强归因请用 worktree 模式） | F5.i |
| S9 | `usage_events.phase` 只写现有 CHECK 允许的 5 个值：规划 `PLANNING`、流水线执行 `EXECUTION`、修复 `REPAIR`、上下文压缩 `CONTEXT`、直接对话 `DIRECT`；角色（含 `REVIEWER` / `TESTER`）记在 `role` 列。订阅制成本写 `equiv_cost_usd`，按量制写 `cost_usd`（06 §3） | F12.b |
| S10 | 默认值改动的设置迁移：新增持久化设置 `settingsVersion`（当前版本 2）；低于 2 时一次性把 `isolation` 置为 `inplace`、`knowledgeAutoUpdate` 置为 `false`。原因：`db.ts` 的 `capture()` 会把默认值也整体写入 SQLite，仅改默认值不会影响已有安装 | F1.g、F8.b |

## 2. 修复顺序总览

| 编号 | 专项 | 级别 | 验证状态 | 依赖 | 主要改动面 |
|------|------|------|----------|------|-----------|
| F0 | 基线与 CI | 前置 | 已确认（无 `.github/`） | — | `.github/`、`scripts/` |
| F1 | 流水线首次派发与默认配置可用 | P0 | 静态推演（测试缺口已确认） | F0.a | `taskRunner.ts`、`execution.ts`、`PlanReview.tsx`、`usePlanSession.ts`、`settings.ts`、`db.ts`、`orchestration/state.rs` |
| F2 | 取消真正终止 CLI | P0 | 已验证（最小复现） | F0.a | `src-tauri/src/lib.rs`、`tokage-native/src/chat/` |
| F3 | 子代理超时与取消入口 | P0 | 已读源码 | F1、F2 | `chat/mod.rs`、`settings.ts`、`OtherSettings.tsx`、`PipelineView.tsx` |
| F4 | write_scope 语义统一 | P1 | 已验证 | — | `tokage-core/command_policy/`、`orchestration/{gate,scheduler}.rs`、`planning/validator.rs` |
| F5 | gate 安全姿态与越界校验 | P1 | 已读源码 + 已验证（allowlist 差异） | F1、F2、F4 | `orchestration/gate.rs`、`native/{child_env,sandbox}`、`taskRunner.ts`、`TasksTable.tsx` |
| F6 | 子代理事件补全与审批 | P1 | 静态推演 | F1、F2、F3 | `taskRunner.ts`、`PipelineView.tsx`、`ipc.ts` |
| F7 | 断点续跑 | P1 | 静态推演 | F1、F6 | `planning/storage.rs`、`execution.ts`、`taskRunner.ts` |
| F8 | 定义包更新入口收口（D6 暂缓） | P1 | 已对照源码 | F1.g | `knowledge.rs`、`lib.rs`、`KnowledgeSettings.tsx` |
| F9 | 调度规则接入 Rust DagScheduler | P1 | 已读源码 | F1、F4、F7 | `orchestration/scheduler.rs`、`lib.rs`、`taskRunner.ts`、`PipelineView.tsx` |
| F10 | 流水线交互收尾 | P2 | 静态推演 | F1、F3、F5、F6、F7 | `PipelineView.tsx`、adapters、`repair.rs` |
| F11 | worktree 隔离完整实现 | P2 | 静态推演 | F1、F5、F7 | `git.rs`、`lifecycle.rs`、`taskRunner.ts` |
| F12 | 数据通路 | P2 | 已读源码与迁移 | F1 | `storage/migrations`、`lib.rs`、`taskRunner.ts` |
| F13 | 执行质量闭环接线 | P2 | 静态推演 | F5、F6、F12 | `taskRunner.ts`、`repair.rs`、`matching` |
| F14 | 学习、分层知识包与节省账本 | P2 | 已读源码 | F12、F13 | `matching`、`storage`、`dashboard.rs`、`stores/knowledge.ts` |
| F15 | Provider 健康与熔断 | P2 | 已读源码 | F9、F12、F13 | `lib.rs`、`native/usage.rs`、`storage/{health,repo}.rs` |
| F16 | 规划与上下文接线 | P2 | 已读源码 | F12、F14 | `planning`、`context`、`Workspace.tsx`、`PlanReview.tsx` |
| F17 | 杂项接线与豁免 | P2 | 脚本统计 | F13–F16 | `Sidebar.tsx`、`lib.rs`、`storage` |
| F18 | 文档与实现对齐 | 收尾 | 直接比对 | F0–F17 | `README.md`、`TODO.md`、`docs/` |

依赖关系的直观理解：F1 不先修，F3 / F6 / F7 的端到端验证起不来（流水线起不来）；F2 的 `chat-ended` 事件被 F6 使用；F4 是 F5（R6）与 F9（写入范围互斥）的前提；F6 的 thread id 是 F7（续跑）、F13（同窗口修复）的前提；F7 的状态 schema 预留了 F9 / F11 要用的字段；F12 的数据通路是 F13–F16 的前提。

## 3. 背景与调查结论

### 3.1 项目与规模

Tokage 是 Tauri 2 桌面应用：不自研 Agent，而是调度本机已装的官方 CLI（Claude Code、Codex、Cursor、Copilot、Antigravity、Aider、OpenCode），先让强模型只读项目并产出精简的设计稿与任务图，再按任务匹配「一次做对概率达标、预期成本最低」的模型并行执行，验收在本地完成（设计文档见 [00](00-architecture-overview/README.md)–[12](12-network-proxy/README.md)）。

- 版本：`master@e291011`（2026-10-06），49 次提交，从 2026-10-04 开始；Rust 约 4.3 万行（含测试），TS / TSX 约 2.6 万行。
- 实际技术栈：Rust（edition 2021，rust-version 1.80）+ Tokio；React 19 + TypeScript 7 + Vite 8 + Zustand 5 + Tailwind 4；SQLite（rusqlite bundled + refinery 迁移 V1–V9）；测试用 `cargo test` 与 `vitest`（`vite.config.ts` 没有 `test` 配置，默认 node 环境，没有 jsdom）。文档里提到的 shadcn/ui、TanStack Query、tree-sitter、tiktoken-rs 实际都没有用（F18 处理文档）。
- 开发入口：`./start_tokage_dev.sh`（带平台与依赖检测的 Tauri 开发启动）、`npm run dev`（纯前端，端口 1420，非 Tauri 环境，`ipc.ts` 返回 mock）、`npm run tauri dev`。

### 3.2 架构地图

```
┌─────────────────────────── Tokage 进程 ───────────────────────────┐
│ WebView：React 19 + Zustand（src/）                                  │
│   lib/ipc.ts          手写 IPC（非 Tauri 环境返回 mock）                │
│   lib/taskRunner.ts   流水线循环：调度、派发、gate、推进都在前端           │
│   stores/execution.ts 流水线状态 ↔ <项目>/.tokage/execution/<会话>.json │
│   lib/db.ts           其余 store ↔ SQLite（300ms 批量 upsert）          │
│        │ invoke（104 个 command）            ▲ 事件：chat-event 等       │
│ ───────┼───────────────────────────────────┼─────────────────────── │
│ Tauri 命令层 src-tauri/src/lib.rs                                       │
│   AppState / Turns / Bus / StorageState                                 │
│   ├─ tokage-native         Adapter ×7 + FakeAdapter、run_turn / run_cli  │
│   ├─ tokage-planning / -matching / -orchestration / -context            │
│   └─ tokage-storage        SQLite（WAL，单写线程 + 读池）                  │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │ spawn（独立进程组、裁剪环境、系统沙箱）
          claude / codex / cursor-agent / copilot / agy / aider / opencode
```

**crate 职责与入口**

| crate | 职责 | 关键入口 |
|-------|------|----------|
| `tokage-core` | 无 IO 的领域模型：事件 `EventKind`、总线 `Bus` / `DomainEvent`、授权 `auth::decide`、命令策略 `command_policy`、计划结构 `planning::{Plan, TaskItem, …}`、代理配置 | `event.rs`、`bus.rs`、`auth.rs`、`planning.rs`、`command_policy/` |
| `tokage-native` | CLI 探测与 Adapter（含 `FakeAdapter`）、执行 `run_turn` / `run_cli`、进程组 `proc`、沙箱 `sandbox`、环境裁剪 `chat/child_env.rs`、git、代理、用量读取、原生会话 | `chat/mod.rs`、`chat/adapter.rs`、`chat/fake.rs`、`proc.rs`、`sandbox/mod.rs`、`git.rs` |
| `tokage-planning` | RepoDigest、计划 prompt / 解析 / 校验 / 落盘 / 重规划、本地分流、`PipelineState` 持久化 | `digest/`、`parser.rs`、`validator.rs`、`storage.rs`、`replan.rs`、`triage.rs`、`prompt.rs` |
| `tokage-matching` | 定义包解析与 5 层合并、CostModel、成功概率、预期成本、全局分配、学习累加器 | `pack.rs`、`merge.rs`、`cost.rs`、`probability.rs`、`expected_cost.rs`、`assignment.rs`、`learning.rs` |
| `tokage-orchestration` | 任务状态机、DAG 调度、简报与报告、gate、修复梯级、生命周期（交付汇总 / worktree 合并）、会话树数据结构 | `state.rs`、`scheduler.rs`、`brief.rs`、`gate.rs`、`repair.rs`、`lifecycle.rs`、`tree.rs` |
| `tokage-context` | 窗口复用、窗口内 CONTINUE / COMPACT / FRESH 决策、压缩载荷、追加消息分流、纪律校验、节省结算 | `reuse.rs`、`in_window.rs`、`compact.rs`、`triage.rs`、`settlement.rs`、`discipline.rs` |
| `tokage-storage` | SQLite（WAL，单写线程 100ms 批量提交，每个 job 独立 savepoint；读连接池 4 个）、迁移前自动备份 | `lib.rs`、`writer.rs`、`repo.rs`、`orchestration.rs`、`learning.rs`、`savings.rs`、`health.rs`、`context.rs`、`knowledge.rs` |
| `tokage`（根 crate） | Tauri 入口与 104 个 command、`AppState`、仪表盘聚合、定义包更新、`open_in`、策略 | `src-tauri/src/{lib,dashboard,knowledge,open_in,policy}.rs` |

**状态与数据存放**

| 位置 | 内容 |
|------|------|
| SQLite `<app data>/tokage.db` | 项目、会话、对话、chat_logs（FTS5）、`settings`（所有设置项整体写入，见 S10）、provider_health、custom_packs（目前至多 1 行）、usage_events、attempts、approvals、command_rules 等（附录 A.13） |
| `<项目>/.tokage/plans/<会话>/` | `spec.md`、`tasks.yaml`（`serde_yaml` 直接序列化 `Vec<TaskItem>`）、`contracts/*.md`、`plan.md`、`history/<版本>.md`；`user_edited/` 子目录；`.git/info/exclude` 自动加 `.tokage/` |
| `<项目>/.tokage/execution/<会话>.json` | 流水线状态（`PipelineState`，原子写：临时文件 + rename） |
| `<app data>/run/children.json` | 运行中的进程组登记，下次启动清理残留（R3） |
| `<app data>/{worktrees,files,knowledge,backups}/` | 会话 / 任务 worktree、轮次写到项目外的文件副本、`pending.json`（待应用的定义包）、迁移前备份 |

**前端不持久化的东西**：`useKnowledge.customPacks`（多个自定义包）与 `modelOverrides`（`db.ts` 的 `capture()` 只持久化 `knowledge.url` 与单个 `custom` 包）；`useExecution` 另走 `.tokage/execution/`。

**Rust → 前端的事件通道**

| 事件 | 发出点 | 前端监听 | 备注 |
|------|--------|----------|------|
| `chat-event` | `forward_to_frontend`（`lib.rs:1172`，只转发 `DomainEvent::ChatOutput`） | `onChatEvent`（`ipc.ts:860`） | 载荷 `ChatEvent`（附录 A.3） |
| `provider-detected` | `refresh_providers`（`lib.rs:84`） | `ipc.ts:492` | 一次扫描里每个 CLI 探测完成时 |
| `provider-melted` / `provider-recovered` | 熔断相关 command（`lib.rs:1642`、`1659`、`1675`） | 无 | F15 |
| `knowledge-pack-update-available` / `knowledge-pack-applied` | `lib.rs:960`、`1007` | 无 | F8 |
| `chat-ended`（新增） | F2.d | F2.d | 轮次结束，含取消 |

`DomainEvent` 现有 7 种：`ProviderDetected`、`ProviderMelted`、`ChatOutput`、`ChatEnded`、`TriageDone`、`TaskStateChanged`、`KnowledgePackUpdateAvailable`（`bus.rs`）；除 `ChatOutput` 外都不会到达前端。

### 3.3 现状运行时流程（带缺陷标记）

**A. 对话轮次**

```
useChat.send → startChatTurn(ChatRequest)                          stores/chat.ts → ipc.ts:871
  → start_chat_turn                                                lib.rs:1063
      外层 spawn（监听总线，记录用量）{ 内层 spawn: run_turn → run_cli }   ← F2：取消只 abort 外层
      run_turn: 校验（trusted / Ready / Direct 限制 / FullAuto 约束）→ run_cli
      run_cli : 沙箱包装 → spawn 子进程（独立进程组、裁剪环境）→ 逐行读 stdout → Adapter 解析 → EventKind
                监督：空闲 / 硬超时（INFINITE 档位和审批挂起时整个跳过）          ← F3
                Ev::File 在只读角色下立即终止（R7）；写入范围越界只发 Warning
                Ev::Denial → decide(...) → Ask 时 emit ApprovalRequest 并置 approval_requested  ← F6
      结束：Done → emit Done；approval_requested 且无 Done → 不发任何终止事件            ← F6
      ChatEnded 只发到总线，前端收不到                                                ← F2
  → Bus → forward_to_frontend → app.emit("chat-event") → stores/chat.ts 或 taskRunner.ts
```

**B. 计划 → 流水线**

```
Plan 模式对话（角色 PLANNER；prompt 由 chat/mod.rs 的 framed_prompt 包装；不含 RepoDigest）  ← F16
  → usePlanSession.reload：loadPlan(.tokage/plans/<会话>/) 或 parsePlanningOutput(助手消息)
  → validatePlan({ plan })        不传 allowed_commands：自动串行化写入冲突、域归一化；不产生命令确认类 issue  ← F5
  → allocatePlanModels(plan, overrides)   只用 builtin 层；结果只用于展示，不写回任务            ← F1.f / F14
  → PlanReview.handleApprove：
       await save()                         // 校验 + persistPlan，返回 void
       startPlanExecution(sessionId, plan.tasks（闭包里的旧值）, …)     ← F1.e
         无依赖任务被预置为 RUNNING                                    ← F1.b
       dispatchReadyTasks(sessionId)        // 候选过滤排除 RUNNING，首批什么都不启动  ← F1.a
  → dispatchSubAgentTask：
       resolveTaskProviderAndModel(task.assigned_model)  // 自动分配的任务该字段为空 → 回退到第一个可运行 CLI  ← F1.f
       isolation=worktree（默认）：createSessionWorktree(baseCwd) 每任务从 HEAD 新建，不含上游产出            ← F1.g / F11
       assembleTaskBrief → startChatTurn({ role: "EXECUTOR", timeoutTier: "INFINITE", writeScope })      ← F3
  → chat-event → handleSubAgentChatEvent
       处理 model / text / toolStart / toolEnd / fileChange / warning / error / done
       忽略 thread / approvalRequest / quota / reasoning                                            ← F6
  → done → processSubAgentCompletion：
       VERIFYING → executeGateCommand(cmd, cwd, 300, true)    // confirmed 恒为 true                  ← F5
       没有 R6 校验、没有修复梯级、没有 attempts / gate_runs / task_outcomes 落库                       ← F5 / F13
       全部通过 → parseOrGenerateTaskReport → recordTaskReport → DONE → progressDownstreamTasks
       失败 → BLOCKED（用户可「重新执行」「终端接管」「标记完成」）                                       ← F10
  → 应用重启：loadPipelineFromDisk 用占位对象重建任务（无依赖 / 目标 / 验收 / 契约 / 产物）               ← F7
```

### 3.4 已做的验证（可复现）

- **E1 取消语义【已验证】**：最小程序复现「外层监听任务 + 内层 spawn 子进程」的结构——abort 外层任务后子进程仍然存活；单层结构（初始提交的写法）下 abort 会杀掉子进程。程序见附录 B.4。引入点：提交 `a92bcd1`（2026-10-05）。
- **E2 write_scope 函数【已验证】**：把 `gate.rs::validate_write_scope` 与 `scheduler.rs::scopes_overlap` 原样抽出运行，结果：`validate_write_scope(["src/server/**"], ["src/server/routes.rs"])` → `passed=false`；`scopes_overlap(["src/server/**"], ["src/server/routes/**"])` → `false`；`scopes_overlap(["src/**"], ["src/server/**"])` → `false`；目录写法 `src/server/` 则判断正确。规划 prompt 要求模型写的正是 `src/server/**` 形式。
- **E3 allowlist 差异【已对照源码】**：定义包 `builtin.json` 的 `gate_allowlist`（14 条，参数只允许 `--flag` 形式）比 `gate.rs::DEFAULT_GATE_ALLOWLIST_PATTERNS`（9 条，尾参数 `(\s+.*)?` 任意，`make <任意目标>`）严格得多；`merge.rs` 把包里的 `gate_allowlist` 合进有效包，但没有任何消费者，gate 只读代码常量。09 §5 写的是「匹配定义包的 `gate_allowlist`」。
- **E4 签名公钥【已对照源码】**：`knowledge.rs:114` 的 `OFFICIAL_MINISIGN_PUBKEY` 与 `minisign-verify` 库 README「Basic Example」里的公钥逐字相同（本机 cargo 缓存 `minisign-verify-0.2.4/README.md:19`、`src/lib.rs`、`benches/bench.rs` 可对照）。
- **E5 接线统计【已验证】**：附录 F 的脚本输出 `commands=104 no_production_caller=48`。
- **E6 环境限制**：见 3.5。

### 3.5 本机验证环境限制（接手者先看）

- 仓库没有 `node_modules`；npm 离线缓存为空（`npm ci --offline` 报 `ENOTCACHED`），所以前端测试在没有联网的环境里跑不起来。
- `cargo --offline` 无法按 `Cargo.lock` 解析（例如 `thiserror` 锁在 2.0.21，本地缓存只有 2.0.19 / 2.0.18），所以 Rust 测试在离线环境里一个都没跑。纯 `std` 的函数可以像 E1 / E2 那样抽到独立 crate 里验证。
- 因此审阅阶段没有运行过两侧的测试套件，文档里「全部测试绿灯」的说法没有证据；F0.a 要在可联网的机器上跑出真实基线。
- 沙箱类测试依赖平台能力（macOS `sandbox-exec`、Linux bubblewrap / Landlock）；依赖真实 CLI 的用例（`chat::live::*`）标了 `#[ignore]`。

### 3.6 接手者容易踩的坑

- **serde 命名不统一**：`TaskItem` 没有 `rename_all`，JSON 键是 snake_case（`depends_on`、`write_scope`、`assigned_model`）；`Plan` / `PlanSpec` / `PlanContract` / `TaskAcceptance` 是 camelCase；`TaskSize` 序列化为 `"S"` / `"M"` / `"L"`；`TaskState` / `TimeoutTier` / `RepairStep` 是 SCREAMING_SNAKE_CASE；`RepairDecision` / `DispatchIneligibleReason` 是外部标签的 camelCase 变体名（如 `{"tryReassign": {…}}`）。
- Tauri command 的 Rust 参数名是 snake_case，TS 调用时用 camelCase（Tauri 自动转换），例如 `timeout_secs` ↔ `timeoutSecs`。
- `ipc.ts` 在非 Tauri 环境（`inTauri` 为 false，vitest 里也是）对很多函数返回 mock（`executeGateCommand` 返回「Mock gate pass」、`startChatTurn` 回放脚本）。写测试必须 `vi.spyOn(ipc, "…")` 覆盖，否则测到的是 mock。
- 设置项整体持久化（见 S10）。新增设置项要同时改：`src/stores/settings.ts`（类型 + 默认值）、`src/components/settings/OtherSettings.tsx`（UI，Execution 卡片在 `:560-581`）、`src/i18n/locales/{en,zh}.ts`（两份都要，`i18next.d.ts` 约束键）。
- `tokage-planning` 依赖 `tokage-native`，反过来不行；需要同时用到两者的拼装放在应用层 `src-tauri/src/lib.rs` 或前端。
- `sandbox::builtin_profiles()` 用 `unwrap_or_default()` 解析以 `ProviderId` 为键的 `sandbox_profiles`：往里加非 `ProviderId` 的键会让整张表解析失败并被吞成空表（所有 CLI 的沙箱画像静默丢失）。新增画像不要放进这张表（F5.f）。
- 表里的 id 类型不统一：`plans.id` / `tasks.id` / `usage_events.plan_id` 是 INTEGER，`gate_runs.plan_id` 与 `attempts.id` 是 TEXT（附录 A.13）。
- SQLite 的 CHECK 约束会让违规写入整条失败，而 `start_chat_turn` 里 `let _ = storage.record_usage(row).await;` 会吞掉错误（附录 A.13、F12）。
- `provider_health.status` 有两个写入方：provider 列表的快照持久化（把检测状态写进 `status`）与熔断命令（写 `MELTED`）；前者会覆盖后者（F15.b）。

## 4. 专项整改

### F0 基线与 CI（前置）

**问题**：审阅时前端依赖未安装、Rust 在离线模式下无法解析 `Cargo.lock`，两侧测试都没能运行；仓库没有 `.github/`，文档里「全部测试绿灯」没有可复核的证据（3.5）。

- [x] F0.a 在可联网的机器上跑一遍基线并记录：`npm ci && npm run typecheck && npm test`；`cd src-tauri && cargo test --workspace --locked`。失败项、`#[ignore]` 用例、依赖真实 CLI 的用例（`chat::live::*`）逐条记入文末「基线记录」。已知风险（均未验证，因为没能编译根 crate）：`tauri.conf.json` 的 `bundle.icon` 列出的 `icons/32x32.png`、`128x128.png`、`128x128@2x.png`、`icon.ico` 在仓库里不存在（只有 `src-tauri/icons/icon.icns`），根 crate 若因此编译失败，先用 `npm run tauri icon` 补齐图标。验证：每项有命令与输出摘要；此条需要联网，先征得同意。
- [x] F0.b 新增 `.github/workflows/ci.yml`：前端作业（Node 22：`npm ci`、`npm run typecheck`、`npm test`）；Rust 作业（macOS + Ubuntu 矩阵：`cd src-tauri && cargo test --workspace --exclude tokage --locked`，Ubuntu 另装 `bubblewrap`）；根 crate 单设一个作业做 `cargo test -p tokage --locked`（`knowledge.rs`、`dashboard.rs` 里有用例，F8 / F12 / F14 / F15 在根 crate 里的测试靠它；Ubuntu 需装 Tauri 的系统依赖，装不上时只在 macOS 跑）。沙箱类测试在不支持的 runner 上必须显式跳过并写明原因，不得静默通过；缓存 cargo 与 npm。验证：故意让一个用例失败的 PR 能让 CI 变红。
- [x] F0.c 新增 `scripts/check-ipc-wiring.py`（口径见附录 F）：列出 `#[tauri::command]` 中在生产 TS 里没有调用方的命令；豁免项写入 `scripts/ipc-wiring-allowlist.txt`（每行 `命令名  理由`）；CI 对「不在豁免表里的无调用方命令」报错。基线 48，目标 0（或全部带理由豁免，F17 收尾）。验证：脚本在当前仓库输出 48；往豁免表加一行后输出 47。
- [x] F0.d 验收：master 上 CI 为绿；基线记录已落盘；`check-ipc-wiring.py` 在 CI 里运行。
- [x] F0.e 文档同步：README 新增「开发与测试」一节（本地跑两侧测试的命令、CI 状态、`check-ipc-wiring.py` 的用法）。

### F1 流水线首次派发与默认配置可用（P0）

**问题与证据**

1. **首批任务不会被派发**【静态推演】：`startPlanExecution` 把无依赖任务预置为 `RUNNING`（`src/stores/execution.ts:114-119`），紧接着的 `dispatchReadyTasks` 把 `RUNNING` 任务排除在候选之外（`src/lib/taskRunner.ts:671-678`；调用序见 `src/components/planning/PlanReview.tsx:140-148`）。现有 E2E 用例是手动 `registerRunningSubAgent` 后合成 `done` 事件，没有覆盖这个序列（`src/lib/taskRunner.test.ts` 的 `m5c: E2E multi-task DAG chain execution`）。
2. **并发派发可能重复拉起同一任务**【静态推演】：`dispatchSubAgentTask` 在两次 `await`（建 worktree、组装简报）之后才置 `RUNNING`（`taskRunner.ts:266-310`），两个任务几乎同时完成时，并发的 `dispatchReadyTasks` 会看到同一个任务仍是 `READY` / `PENDING`。失败路径还会泄漏 `activeSubAgents` 里的 ctx，任务状态停在 `RUNNING`（`handleRetryTask` 是 `void dispatchSubAgentTask(...)`，没有 `catch`，`PipelineView.tsx:105-117`）。
3. **校验器的修复丢失**【静态推演】：`save()` 校验后持久化的是 `val.plan`（含自动串行化加的依赖边、域归一化），但 `handleApprove` 在 `await save()` 之后用的是闭包里的旧 `plan`（`PlanReview.tsx:133-149`；`usePlanSession.ts:194-209` 的 `save` 返回 `void`）。用户未编辑就直接确认时，流水线拿到的是未校验的任务。
4. **自动分配结果没有写回任务**【静态推演】：`runAllocation` 只把结果放进 `allocation` 状态（`usePlanSession.ts:247-267`），`task.assigned_model` 只有手动选择才会设置（`PlanReview.tsx:404-413`、`TasksTable.tsx:676-708`）。没手选的任务走 `resolveTaskProviderAndModel(null)` 的「第一个可运行 CLI 的第一个模型」兜底（`taskRunner.ts:188-201`），匹配引擎的选择不影响真实执行。
5. **默认隔离让下游看不到上游产出**【静态推演】：默认 `isolation: "worktree"`（`src/stores/settings.ts:135`），每个任务从 `HEAD` 新建 worktree 且不含未提交改动（`git.rs:202`、`taskRunner.ts:266-278`）。03 §6 写的默认是 `shared`（代码取值叫 `inplace`，`IsolationMode = "worktree" | "inplace"`）。设置会连同默认值整体持久化（`db.ts` 的 `capture()`），只改默认值不会影响已有安装（S10）。
6. **前端状态不校验转换**【静态推演】：`updateTaskState` 直接覆盖状态（`execution.ts:156-213`），Rust 的转换表（`state.rs:57-117`）在前端没有对应；`handleRetryTask` 直接 `BLOCKED → RUNNING`，表里没有这条；03 §8 要求「终端接管完成后跑验收」，表里也没有 `BLOCKED → VERIFYING`。

**目标契约**

```ts
// 单一派发入口：取代 PlanReview / progressDownstreamTasks / PipelineView 里各自的派发逻辑
export async function dispatchReadyTasks(
  sessionId: string,
  opts?: { only?: string[] },        // 手动重试 / 继续时只派发指定任务
): Promise<string[]>;                // 返回已拉起的 turnId

// dispatchSubAgentTask 的同步前置（先于任何 await）：
//   1. READY → RUNNING（写 turnId、startedAt、日志）
//   2. registerRunningSubAgent(turnId, ctx)
// 之后才允许 await（建 worktree、组装简报、startChatTurn）；任一步失败 → BLOCKED(原因) + unregisterRunningSubAgent(turnId)
```

- 初始状态：无依赖任务 `READY`，其余 `PENDING`；`PENDING → READY` 在派发入口里完成（依赖全部 `DONE`）。F9 之后改由 Rust 的 `promoted` 输出决定。
- `assigned_model` 规范格式 `<ProviderId>:<cliModelArg>`，例如 `CLAUDE_CODE:sonnet`（手动选择产生的 `<ProviderId>:<目录里的变体 id>` 同形，解析器两种都认）。分配结果的完整快照 `TaskAssignment`（附录 A.12）随流水线状态保存。
- `resolveTaskProviderAndModel(spec, …)` 返回 `ResolvedModel | null`；`null`（目录里没有该 CLI / 该模型、CLI 未启用 / 未信任 / 不可运行）→ 任务 `BLOCKED`，原因写明；删除「第一个可运行 CLI」与「默认 CLAUDE_CODE」两处兜底（`taskRunner.ts:188-211`）。熔断后的自动改派由 F15 负责。
- 状态转换表：前端新增 `src/lib/taskState.ts`（`TASK_STATE_TRANSITIONS`、`canTransition(from, to)`），与 Rust `TaskState::can_transition_to` 同源校验（金文件，见 F1.d）；转换表见附录 A.5。
- 设置：`IsolationMode` 取值不变，默认值改为 `"inplace"`；新增持久化设置 `settingsVersion: number`（默认 2）。

**整改步骤**

- [x] F1.a 复现用例（先红）：`src/lib/taskRunner.test.ts` 新增「确认计划后派发」——先给 `useProviders` / `useSettings` 灌入 provider 与信任的 fixture，`startPlanExecution([T1, T2(依赖 T1)])` → `vi.spyOn(ipc, "startChatTurn")` → `dispatchReadyTasks(session)`；断言 `startChatTurn` 恰好被调用 1 次、turnId 以 `subagent-<session>-T1-` 开头、T1 为 `RUNNING`、T2 仍为 `PENDING`。现状预期失败（调用 0 次）。验证：`npx vitest run src/lib/taskRunner.test.ts`。
- [x] F1.b 修复：`startPlanExecution` 不再预置 `RUNNING`，无依赖任务置 `READY`（初始日志改为「[就绪] 等待调度」，`startedAt` 不再预置）；`RUNNING` 只在 `dispatchSubAgentTask` 认领时设置。同步修正 `m5b` / `m5c` 等用例里对 `taskStates` 的期望。
- [x] F1.c 并发与失败兜底：复现用例——同一 session 并发 `await Promise.all([dispatchReadyTasks(s), dispatchReadyTasks(s)])`，断言每个任务只 `startChatTurn` 一次；`startChatTurn` 抛错时任务落 `BLOCKED` 且 `getRunningSubAgent(turnId)` 为 `undefined`；建 worktree 失败同理。修复：把「`READY → RUNNING` + 生成 turnId + 注册 ctx」前移到 `dispatchSubAgentTask` 所有 `await` 之前；任一 `await` 失败走统一的收尾函数（落 `BLOCKED`、记日志、注销 ctx）。
- [x] F1.d 状态转换校验：① Rust `state.rs` 新增 `BLOCKED → VERIFYING` 与 `WAITING_UPSTREAM → SKIPPED`（S1）；② 新增金文件 `src-tauri/crates/tokage-orchestration/tests/fixtures/task_state_transitions.json`（`{ "PENDING": ["READY", …], … }`），Rust 测试断言 `can_transition_to` 算出的表与金文件相等；③ 前端 `src/lib/taskState.ts` 手写同一张表，测试读金文件逐项比对；④ `updateTaskState` 拒绝非法转换（`console.warn` + 在任务日志追加「[状态机] 拒绝非法转换 X→Y」，状态不变），`startPlanExecution` / `loadPipelineFromDisk` 直接设初始状态不受限。已有用例里从 `RUNNING` 直接到 `DONE` 的写法要改成经 `VERIFYING`。
- [x] F1.e 用校验后的计划驱动执行：复现用例——`vi.spyOn(ipc, "validatePlan")` 返回加了串行化依赖边的 `val.plan`，用户未编辑直接确认后，`useExecution` 里的任务带有该依赖边。修复：`usePlanSession.save()` 返回校验后的 `Plan | null`，`handleApprove` 用它（而不是闭包里的 `plan`）调用 `startPlanExecution`，并用于生成「首批并行执行任务」的提示文案。
- [x] F1.f 分配结果写回与解析：复现用例——`allocatePlanModels` 返回 `assignments`，确认计划后未手选的任务 `assigned_model` 被填成 `<provider>:<cliModelArg>`；`resolveTaskProviderAndModel` 对目录里不存在的模型返回 `null` 并使任务 `BLOCKED`（原因写明）。修复：`handleApprove` 在确认时重新计算一次分配（`overrides` = 手选），写回任务并持久化计划；把 `assignments` 快照传给 `startPlanExecution`；按上文契约改 `resolveTaskProviderAndModel`；更新 `taskRunner.test.ts` 里依赖兜底行为的用例（`falls back to another runnable provider if the assigned provider is disabled` 改为断言返回 `null`）。
- [x] F1.g 默认隔离止血与设置迁移：复现用例——① 设置迁移函数 `migrateSettings(values)`：缺少 `settings.settingsVersion` 且 `settings.isolation === "worktree"` → 变为 `"inplace"`，`settings.knowledgeAutoUpdate` 变为 `false`，版本置 2；已是版本 2 的值保持不变；② 默认设置下 `isolation === "inplace"`。修复：`settings.ts` 默认值与 `settingsVersion`；`db.ts` 的 `apply()` 在 `useSettings.setState` 前调用 `migrateSettings`；`OtherSettings.tsx` 把 `worktree` 选项标为「实验性：完成 F11 前不会合并回主仓库」（`i18n` 的 `settings.execution.isolationModes.worktree` 文案同步，中英文两份）。
- [ ] F1.h 统一派发入口：`PlanReview.handleApprove`、`progressDownstreamTasks`、`PipelineView.handleRetryTask` 全部经 `dispatchReadyTasks(sessionId, { only })`（暂停检查、并发上限、失败兜底只写一份）。`handleRetryTask` 改为先 `updateTaskState(…, "READY")`（`BLOCKED` / `INTERRUPTED` → `READY` 是合法转换）再派发，并 `.catch` 记日志；不再硬编码 `useWorktree: true`（隔离模式读设置）。
- [ ] F1.i 验收：上述用例全绿；手动端到端（FakeAdapter 或真实 CLI，默认设置）——确认计划 → 每个根任务各出现一个子进程（`ps` 可见）→ 完成后 gate → 下游自动启动且读得到上游写入的文件；自动分配的任务确实用了分配给它的 CLI 与模型；同时运行的任务数不超过 `maxParallel`。
- [ ] F1.j 文档同步：03 §6（隔离模式命名统一为 `inplace` / `worktree`，默认 `inplace`，`worktree` 标注实验性）、03 §2（状态图加 `BLOCKED → VERIFYING`）、02（确认计划时把分配结果写回 `assigned_model`）。

### F2 取消真正终止 CLI（P0）

**问题与证据**【已验证】：`start_chat_turn` 为了在总线上记录用量，把 `run_turn` 包进了内层 `tokio::spawn`（`src-tauri/src/lib.rs:1092`，引入于提交 `a92bcd1`）；`cancel_chat_turn` 只 abort 外层监听任务（`lib.rs:1159-1169`）。丢弃 `JoinHandle` 不会取消内层任务，持有子进程与 `proc::Group` 的任务继续运行：CLI 不会被终止，仍占用额度、继续改文件；`sendQueuedNow` 取消后立刻派发新消息，会在同一目录并发两个 CLI（`src/lib/dispatch.ts:50-61`）。已有的 `hang_is_killed_on_stop`（`src-tauri/crates/tokage-native/src/chat/fake.rs:518-531`）是用 `timeout()` 丢弃单层 future 来测的，没经过命令层，所以没发现。`Group::drop` 会 SIGTERM 进程组并在 3 秒后由独立线程补 SIGKILL（`proc.rs:394-431`），所以只要持有它的任务被 drop / abort 就会回收——问题只在 abort 的不是那个任务。

**目标契约**

```rust
// tokage-native::chat::turns（新增模块）：取消与登记放在 native 里，便于用 FakeAdapter 测
pub struct TurnRegistry { /* Mutex<HashMap<turn_id, TurnEntry>> */ }
impl TurnRegistry {
    /// 登记并启动一个轮次：只 spawn 一个任务，run_turn 直接在其中运行；
    /// 任务结束时（正常 / 失败 / 取消）恰好发布一次 DomainEvent::ChatEnded { turn_id, cancelled }。
    pub fn spawn(&self, bus: Bus, req: ChatRequest, providers: Vec<ProviderInfo>, env: ShellEnv, mirror: Option<PathBuf>);
    /// 发出取消信号并等待轮次任务结束（含进程组确认退出）；超过 8 秒则 abort 兜底。返回该轮次此前是否在运行。
    pub async fn cancel(&self, turn_id: &str) -> bool;
}
// run_cli 增加取消参数：读循环 select! 到取消 → group.kill().await + child.kill().await → 返回「已取消」变体。
// 取消不发 EventKind::Error（前端的 error 分支会把流水线任务置 BLOCKED）。
```

- `DomainEvent::ChatEnded` 增加 `cancelled: bool`；`forward_to_frontend` 把它转发为 Tauri 事件 `chat-ended`，载荷 `{ turnId: string, cancelled: boolean }`；`ipc.ts` 新增 `onChatEnded(cb)`。
- 用量监听作为独立任务订阅总线，收到本轮的 `ChatEnded` 即结束；`start_chat_turn` / `cancel_chat_turn` 变成薄封装。
- `cancel_chat_turn(turn_id)` 返回时进程组已确认退出。

**整改步骤**

- [ ] F2.a 重构（不改行为）：新增 `chat/turns.rs`，把登记、spawn、用量监听拆开；`run_cli` 增加取消参数（测试辅助里 3 处调用同步更新）；`start_chat_turn` 改为薄封装。验证：现有 `cargo test -p tokage-native` 保持全绿。
- [ ] F2.b 复现用例（先红）：在 `fake.rs` 的测试模块里（那里有 `env` / `pid` / `gone` 辅助函数，附录 B.1），用 `Step::Hang { pidfile }` 经 `TurnRegistry::spawn` 启动 → 读 pidfile → `cancel` → 断言限定时间内该 pid 及其进程组消失；同时断言 `ChatEnded` 恰好一次且 `cancelled == true`、取消之后总线上不再有该 turn 的 `ChatOutput`；再加一条正常结束的用例断言 `cancelled == false`。验证：`cd src-tauri && cargo test -p tokage-native cancel`。
- [ ] F2.c 修复：协作式取消（`watch` / `Notify` 信号），`abort()` 只作 8 秒超时兜底；`Turns` 登记换成 `TurnRegistry`；用量监听独立成任务。
- [ ] F2.d 终止事件：`ChatEnded { turn_id, cancelled }` 转发为 `chat-ended`；前端新增 `onChatEnded`。复现用例：FakeAdapter 在审批挂起后无 `done` 退出（`Step::Denial` + `Step::Exit(0)`），总线上依次出现 `ApprovalRequest`、`ChatEnded`（F6.b 用它判定「结束但无终止事件」）。
- [ ] F2.e 前端取消语义：`src/stores/chat.ts` 的 `cancel` 先 `await cancelChatTurn` 成功再标 `cancelled`，失败时给出提示；`sendQueuedNow`（`src/lib/dispatch.ts`）只在取消完成后才派发新消息。
- [ ] F2.f 验收：F2.b / F2.d 用例转绿，`hang_is_killed_on_stop`、`grandchildren_are_reaped` 保持全绿；手动——对话中点「停止」后 `ps` 无残留进程；「立即发送」期间只存在一个 CLI 进程。
- [ ] F2.g 文档同步：09 §2.3（取消走协作式回收，等待进程组退出后返回）、00 §6 的 `ChatEnded` 载荷。

### F3 子代理超时与取消入口（P0）

**问题与证据**【已读源码】：子代理轮次用 `timeoutTier: "INFINITE"`（`taskRunner.ts:340`），`run_cli` 的监督循环在 `approval_requested || limits.infinite` 时整段跳过空闲与硬超时（`chat/mod.rs:759-763`）：卡死的 CLI 会永远占着并发槽位，而流水线没有任何取消入口——「暂停调度」只停新任务（`PipelineView.tsx:248-275`），`cancelChatTurn` 只被主聊天的取消按钮调用。此外 `approval_requested` 对所有档位都豁免了硬超时，而 09 §2 只说「等待审批时暂停空闲计时」。

**目标契约**

```rust
pub enum TimeoutTier { Quick, Standard, Long, Planning, Gate, Agent }   // 删除 Infinite
impl TimeoutLimits { pub fn agent(idle: Duration, hard: Duration) -> Self }   // Agent 档位不乘 timeout_multiplier
// ChatRequest 新增（camelCase：idleTimeoutSecs / hardTimeoutSecs）：
//   idle_timeout_secs: Option<u64>   缺省 1800，仅 Agent 档位使用
//   hard_timeout_secs: Option<u64>   缺省 7200，仅 Agent 档位使用
```

- 监督语义（所有档位）：空闲计时在每行 stdout 到达时重置，等待审批期间暂停；硬计时从轮次开始起算，**任何情况下都不暂停**。超时 → `group.kill()` → `Err("Command timed out (idle|hard timeout exceeded)")`（消息格式保持不变，前端据此识别）。
- 设置（默认值即 D5）：`agentIdleTimeoutMin = 30`、`agentHardTimeoutMin = 120`；约束 `agentHardTimeoutMin ≥ agentIdleTimeoutMin`（设置时若冲突，把硬上限抬到等于空闲值）；UI 在 `OtherSettings.tsx` 的 Execution 卡片（`:560-581`）加两个 `NumberInput`，范围建议 idle 1–480（步长 5）、hard 5–1440（步长 10）；i18n 中英文各加 `settings.execution.agentIdle` / `agentHard` 及描述。
- 取消任务（S2）：

```ts
export async function cancelTask(sessionId: string, taskId: string): Promise<void>;
//  RUNNING：await cancelChatTurn(turnId) → CANCELLED
//  VERIFYING：置 CANCELLED，gate 结果返回后被丢弃（F5.e 之后改为真正终止 gate 进程）
//  其他状态：忽略。随后：传递下游中状态为 PENDING / READY / WAITING_UPSTREAM 的任务 → SKIPPED（日志注明来源任务）；unregisterRunningSubAgent(turnId)
//  迟到事件守卫：handleSubAgentChatEvent 与 processSubAgentCompletion 在每个 await 之后重新读取任务状态，
//  不是预期状态（RUNNING / VERIFYING）就放弃后续转换；终态任务的事件只记日志。
```

**整改步骤**

- [ ] F3.a 复现用例（Rust，先红）：`TimeoutTier::Agent` 下——① 无输出超过 idle 即被终止；② 持续有输出但超过 hard 也被终止；③ 审批挂起后（`Step::Denial`）超过 hard 仍被终止，而 idle 计时暂停（挂起期间无输出不触发 idle）。用 `TOKAGE_IDLE_TIMEOUT_MS` / `TOKAGE_HARD_TIMEOUT_MS` 环境变量覆盖（`TimeoutLimits::resolved` 已支持）把时间缩到毫秒级。验证：`cargo test -p tokage-native timeout`。
- [ ] F3.b Rust：用 `Agent` 取代 `Infinite`；`ChatRequest` 增加 `idle_timeout_secs` / `hard_timeout_secs`；监督循环删除 `limits.infinite`，`approval_requested` 只暂停空闲计时；`ipc.ts` 的 `timeoutTier` 类型换成 `"AGENT"` 并加两个秒数字段。
- [ ] F3.c 前端设置与派发：`settings.ts` 增加两个设置项与约束；`OtherSettings.tsx` + i18n；`dispatchSubAgentTask` 传 `timeoutTier: "AGENT"` 与秒数（派发时读设置）。复现用例：改设置后新派发的请求携带新的秒数。
- [ ] F3.d 取消任务（复现用例 + 实现）：`cancelTask` 与看板「取消任务」按钮；用例——取消 `RUNNING` 任务后 `cancelChatTurn` 被调用、任务 `CANCELLED`、下游 `SKIPPED`、之后到达的 `done` 事件不再触发 gate / `DONE`。
- [ ] F3.e 超时落地：`error` 事件的消息含超时类型时，任务置 `BLOCKED`，原因写「空闲超时（N 分钟）/ 硬超时（N 分钟）」，日志保留最后输出与已改动文件清单（09 §2.4：被终止的 CLI 可能已改动部分文件，清单写入失败证据）。provider 连续失败计数在 F15 接入。
- [ ] F3.f 验收：F3.a / F3.c / F3.d 用例全绿；手动——流水线里的 FakeAdapter 挂起任务在设定的 idle 后变 `BLOCKED` 并写明原因；点「取消任务」后 `ps` 无残留、下游变 `SKIPPED`。
- [ ] F3.g 文档同步：09 §2.1 表新增 `AGENT` 行（idle / hard 来自设置，默认 30 / 120 分钟），并把 `PLANNING` 行改成与代码一致的 300 s / 900 s；09 §2.2 实现片段改为硬超时不因审批暂停；03 §2 补充取消与 `SKIPPED` 语义。

### F4 write_scope 语义统一（P1）

**问题与证据**【已验证】：见 3.4 的 E2。除此之外：

- `tokage-planning` 的 `globs_overlap`（`validator.rs:451`）是字符串 `starts_with`：`src/a/**` 与 `src/ab/**` 被误判为重叠（保守方向，损失并行度），`**/*.ts` 与 `src/x.ts` 漏判（危险方向）。
- `tokage-core` 已有按路径段实现的 `command_policy::path::{glob, overlap, under, literal_base}`（`path.rs`，支持 `**`、`*`、`?`、`[..]`，尾部 `/**` 同时覆盖目录自身）。
- `classify_edit`（`classify.rs:1354-1362`）对无通配符的条目只做精确匹配，而 orchestration 按目录前缀匹配——三处语义互不一致。
- `validate_write_scope` 目前没有被接线所以没爆；F5.i 接线后，配合 `git checkout --` 会把合法改动还原掉。

**目标契约**（D4-A）

```rust
// tokage-core/src/command_policy/scope.rs（新增，导出为 command_policy::scope）
pub fn normalize_scope(entry: &str) -> String;            // 去 "./"、反斜杠→"/"、折叠重复 "/"、去尾部 "/"；`.` 与空串→"**"（整个工作区）；无通配符（* ? [）→ "<entry>/**"
pub fn covers(scope: &str, file: &str, fold_case: bool) -> bool;          // path::glob(normalize_scope(scope), normalize_path(file))
pub fn any_covers(scopes: &[String], file: &str, fold_case: bool) -> bool;
pub fn overlap(a: &str, b: &str, fold_case: bool) -> bool;                // path::overlap(normalize_scope(a), normalize_scope(b))
pub fn scopes_overlap(a: &[String], b: &[String], fold_case: bool) -> Option<(String, String)>;
// fold_case：macOS / Windows 为 true（沿用 Context.case_insensitive 的取值）
```

- 规范化后的条目永远是 glob；旧计划里已落盘的目录写法无需迁移（`covers` / `overlap` 内部先 `normalize_scope`）。
- 行为变化（有意）：`classify_edit` 对 `src/users/` 这类目录写法，目录内文件不再被整体判为 `outside_write_scope`。

**整改步骤**

- [ ] F4.a 复现用例（先红）：为 `scope::{covers, overlap}` 与四处旧实现共用一份表驱动用例——`src/server/**` 对 `src/server/routes.rs`（应覆盖）；目录写法 `src/users/` 对 `src/users/a/b.rs`（应覆盖）；单文件 `src/main.rs`；`**/*.ts` 对 `a/b/c.ts`；根下 `*.md`；`src/a/**` 与 `src/ab/**`（不应重叠）；`src/**` 与 `src/server/**`（应重叠）；`**/*.ts` 与 `src/x.ts`（应重叠）；Windows 反斜杠；`fold_case` 开关；点目录 `.github/**`；`.` 与空串（整个工作区，规范化为 `**`）。验证：`cargo test -p tokage-core scope`。
- [ ] F4.b 实现 `scope` 模块（契约如上）。
- [ ] F4.c 替换各处实现：`orchestration::gate::validate_write_scope`、`orchestration::scheduler::scopes_overlap`、`planning::validator::{find_scope_overlap, globs_overlap}` 改调 `scope::*`；删除旧的字符串实现。既有 `validator.rs`、`scheduler.rs` 用例保持全绿。
- [ ] F4.d `classify_edit`（`classify.rs:1354-1362`）的 write_scope 判定改用 `scope::covers`，并在 `command_policy/tests.rs` 补一条目录写法的用例。
- [ ] F4.e 解析阶段规范化：`tokage-planning` 的 parser 与 validator 输出的 `write_scope` 经 `normalize_scope`（`validate_plan` 返回的 `plan` 已是被修复过的计划，直接在那里规范化）；`TasksTable.tsx` 的编辑框保持原样输入，保存时由 `validatePlan` 规范化。
- [ ] F4.f 验收：F4.a 全绿；既有用例全绿；对 `src/**` 与 `src/server/**` 的并行任务，`validate_plan` 自动加依赖串行化；`orchestration_acceptance.rs` 的 `test_acceptance_p7_m3_gate_execution_and_scope_validation` 保持绿。
- [ ] F4.g 文档同步：11 §3（write_scope 语义与 `classify_edit` 的行为变化）、02 plan-format（`write_scope` 写法与规范化规则）、09 §4。

### F5 gate 安全姿态与越界校验（P1）

**问题与证据**

1. `userConfirmed` 写死为 `true`【已读源码】：两个生产调用点（`taskRunner.ts:481`、`PipelineView.tsx:193`）都传 `true`，R5 allowlist 被绕过，只剩元字符过滤（`gate.rs:46-98`）。封装的默认值是 `false`（`ipc.ts:2258-2279`）。
2. allowlist 来源错误【已对照源码】：见 E3；`check_gate_command_safety` 只用代码常量（`gate.rs:24-34`），不读定义包。
3. 完整环境、无沙箱【已读源码】：`execute_gate_command` 用 `env_clear().envs(&env.vars)` 传入完整登录环境（含各家 API Key），且不经系统沙箱；gate 会运行模型写的测试和构建脚本（`gate.rs:166-175`），等于绕过了 CLI 窗口上的沙箱与环境裁剪（R14 / R16）。
4. 超时只杀直接子进程【已读源码】：超时只 drop `Child`（`kill_on_drop`，`gate.rs:174,191`），孙进程残留，也没有 `proc::isolate` / `Group`（R3）；`wait_with_output` 全量缓冲输出（R1）。
5. 验收命令并发【已读源码】：09 §3 规定「验收命令并发 1」，代码里没有任何串行化，并行任务的 gate 会同时跑。
6. R6 未接线【已读源码】：`validate_write_scope_cmd`、`revert_out_of_scope_files_cmd` 无调用方；`revert_out_of_scope_files` 用 `git checkout --`，对未跟踪文件会整条命令失败（git 语义，待用例确认），对用户已有的未提交改动则会误伤。
7. 计划审阅看不到命令风险【已读源码】：`usePlanSession` 调 `validatePlan({ plan })` 不传 `allowed_commands`，`unverified_acceptance_command` 与 `acceptance_auto_fill` 都在 `if !opts.allowed_commands.is_empty()` 内（`validator.rs:204,223`），生产里从不触发；审阅界面没有逐条确认。

**目标契约**

```rust
// tokage-orchestration/src/gate.rs
pub struct GateRequest<'a> {
    pub command: &'a str,
    pub workdir: &'a Path,
    pub env: &'a ShellEnv,
    pub timeout_secs: Option<u64>,        // 缺省 300
    pub confirmed: bool,                  // = 任务 acceptance.confirmed_commands 含该命令
    pub allowlist: &'a [String],          // 有效定义包的 gate_allowlist
    pub env_allow: &'a [String],          // 设置 gateEnvAllow
    pub run_id: Option<&'a str>,          // 取消用；None 则不可取消
}
pub async fn execute_gate_command(req: GateRequest<'_>) -> GateCommandResult;
pub fn check_gate_command_safety(cmd: &str, confirmed: bool, allowlist: &[String]) -> GateCommandPermission;
// R5：含元字符 → Denied（不可确认）；匹配 allowlist → 允许；否则 confirmed → 允许；否则 Denied。
// IPC：execute_gate_command_cmd(command, workdir, timeout_secs?, confirmed, env_allow?, run_id?) -> GateCommandResult
//      cancel_gate_command(run_id) -> bool
//      allowlist 由命令内部从有效包取（F14 之前是内置包）；删除 DEFAULT_GATE_ALLOWLIST_PATTERNS

// tokage-core/src/planning.rs：TaskAcceptance 新增（camelCase：confirmedCommands，#[serde(default)]）
pub confirmed_commands: Vec<String>,
```

- 校验器新规则（`PlanValidationOptions` 增加 `gate_allowlist: Vec<String>`，`validate_plan` 命令里从有效包取，前端不用传）：验收命令含元字符 → `Error` `unsafe_acceptance_command`（不可确认）；不在 allowlist 且不在 `confirmed_commands` → `Error` `unconfirmed_acceptance_command`；不在 allowlist 但已确认 → `Info` `acceptance_command_confirmed`。`PlanReview` 已有 `errorCount > 0` 阻止确认的逻辑，直接复用（`PlanReview.tsx:133-134`）。`unverified_acceptance_command`（项目已知脚本列表）保留给 F16 的 RepoDigest。
- gate 环境（`child_env::assemble_gate_env(env, allow) -> HashMap<String, String>`）：从登录环境出发，删除命中 `is_credential` 的变量、删除值里含 `://user:pass@`（userinfo）的变量，但 `allow` 里列出的名字（精确名或 `PREFIX_*`）保留。
- gate 沙箱画像在代码里定义（`sandbox::gate_profile()`），**不要**放进 `builtin.json` 的 `sandbox_profiles`（见 3.6）。内容：`write` = `{worktree}`、`{tmp}`、`~/.npm/**`、`~/.cache/**`、`~/.cargo/registry/**`、`~/.cargo/git/**`、`~/.cargo/.package-cache`、`~/go/pkg/mod/**`、`~/.gradle/caches/**`、`~/.m2/repository/**`、`~/Library/Caches/**`、`~/Library/pnpm/**`、`~/.local/share/pnpm/**`、`~/.bun/install/cache/**`；`read` / `deny` / `keychain` 取 `CLAUDE_CODE` 画像去掉 `~/.claude*` 后的内容（`deny` 里的 `{other_cli_dirs}` / `{forbidden_paths}` 因此不再豁免任何 CLI 的目录），`keychain = false`。网络不限制（D3）：SBPL 默认不禁网，bwrap 不带 `--unshare-net`，Landlock 无网络规则。写入清单以实测为准——首次实现要用真实项目的 `npm test` / `cargo test` / `pytest` 各跑一遍确认不缺写权限。

**整改步骤**

- [ ] F5.a 复现 / 契约用例（先红，Unix）：① 父环境带 `ANTHROPIC_API_KEY=x` 与带 userinfo 的 `HTTPS_PROXY` 时，gate 运行的探针脚本（`./printenv.sh`，`confirmed = true`）输出里不得出现它们；`env_allow` 含 `ANTHROPIC_API_KEY` 时才出现；② 超时用例：脚本 `./leak.sh` 在后台留下 `sleep 300`，超时后断言该孙进程已消失；③ 输出洪泛：命令输出 200 MB 时内存有界、只保留尾 200 行；④ allowlist：`make deploy` 被拒（常量会放行，定义包只放行 `test|build|check|lint|all`）、`npx tsc` 放行、含 `|` 的命令即使 `confirmed` 也被拒；⑤ 串行：两个并发 gate 的执行区间不重叠（用标记文件的时间戳）；⑥ 沙箱（macOS 与 Linux，沿用 `sandbox_intercepts_credential_redline_reads` 的写法）：gate 里读 `~/.ssh` 被拦截、写 worktree 外被拦截、写 worktree 与 `~/.cargo/registry` 放行、出站网络放行。验证：`cargo test -p tokage-orchestration gate` 与 `cargo test -p tokage-native sandbox`。
- [ ] F5.b allowlist 来源统一：`check_gate_command_safety` 与 `execute_gate_command` 按契约改签名；`execute_gate_command_cmd` 从有效包取 `gate_allowlist`；删除 `DEFAULT_GATE_ALLOWLIST_PATTERNS` 与 `custom_patterns` 参数；更新 `orchestration_acceptance.rs` 里受影响的用例。
- [ ] F5.c 用户确认（S3）：`TaskAcceptance` 增加 `confirmed_commands`（TS 类型同步）；`PlanValidationOptions.gate_allowlist` 与上述三条规则；`TasksTable.tsx` 任务详情的验收命令列表（`:869-885`）每条显示状态徽章——白名单（绿）/ 已确认（蓝）/ 未确认（橙，带「确认」按钮，点击把命令加入 `acceptance.confirmed_commands` 并 `saveUserEdited`）；`taskRunner` 的 gate 调用传 `confirmed = task.acceptance.confirmed_commands.includes(cmd)`，删除两处硬编码 `true`。复现用例：未确认且不在白名单的命令 → `validate_plan` 返回 `unconfirmed_acceptance_command` Error；确认后变 Info；gate 对未确认命令返回 Denied 并使任务 `BLOCKED`。
- [ ] F5.d 环境裁剪：实现 `child_env::assemble_gate_env`；设置项 `gateEnvAllow: string[]`（默认 `[]`）加入 `settings.ts`，`OtherSettings.tsx` 的 Execution 卡片加一个文本域（每行一个名字）与警示文案，i18n 中英文各一份；`execute_gate_command_cmd` 接收 `env_allow`。
- [ ] F5.e 进程隔离与可取消：gate 子进程使用 `proc::isolate` 与 `proc::Group`，超时 / 取消时整组回收并登记 `children.json`；新增 `GateRuns` 注册表与 `cancel_gate_command(run_id)`；`cancelTask`（F3.d）对 `VERIFYING` 任务改为调用它。
- [ ] F5.f 系统沙箱（D3）：重构 `build_sbpl_for_turn` / `build_landlock_profile_for_turn` / `build_bwrap_args_for_turn` 使其接收 `&SandboxProfile`（保留按 `ProviderId` 取画像的外壳）；新增 `sandbox::gate_profile()` 与 `create_sandboxed_gate_command`；gate 在 macOS / Linux 上经它运行；无沙箱平台（Windows、Linux 上 bwrap 与 Landlock 都不可用）退化为未沙箱运行并在 gate 结果与 UI 里提示。
- [ ] F5.g 输出：流式读取 stdout / stderr，只保留尾 200 行并经 `OutputFilter` 脱敏（R1 / R17），不再 `wait_with_output` 全量缓冲。
- [ ] F5.h gate 串行：全局 `tokio::sync::Semaphore(1)`，在 `execute_gate_command` 内获取；等待期间任务保持 `VERIFYING` 并在日志里写「排队等待验收」。
- [ ] F5.i R6 接线（依赖 F4）：任务轮次结束后、gate 之前，取本轮的改动文件集（`ctx.changedFiles`，来自 `fileChange` 事件，含轮次结束时对 `git status` 的补扫，`files.rs::finish`；FileChange 的 `path` 在工作目录内是相对路径，在目录外是绝对路径，目录外一律视为越界），调用 `validateWriteScope(task.write_scope, changedFiles)`；`inplace` 模式下按 S8 排除「与本任务运行时间重叠的其他任务的 write_scope」覆盖的文件（实现：用 `taskStartedAt` / `taskEndedAt` 求时间重叠的任务）；越界 → gate 不执行，任务 `BLOCKED`，证据列出越界文件（`FailureEvidence.out_of_scope_files`）；看板提供「一键还原越界文件」（调用 `revertOutOfScopeFiles`）。复现用例：任务改了 scope 外的文件 → 被阻塞且证据列出该文件；`inplace` 下两个并行任务各改自己 scope 内的文件 → 都通过。worktree 模式（F11.h）改用 `git diff --name-only <基线>` 作为权威集合并与事件集合取并集。
- [ ] F5.j 修正 `revert_out_of_scope_files`：已跟踪文件 `git checkout -- <files>`，未跟踪文件另行删除（`git clean -f -- <files>`），两类分开处理；只作用于任务 worktree（隔离模式）或有基线快照的共享模式（`git::record_turn_snapshot`，`git.rs:344`），避免误伤用户已有的未提交改动；执行前由 UI 二次确认（09 §4）。复现用例：混合已跟踪 + 未跟踪文件，且工作区里另有无关的未提交改动，还原后只有越界文件被处理。
- [ ] F5.k 验收：F5.a 用例全绿；R5 / R6 / R16 各有一条经 `processSubAgentCompletion` 的生产路径级测试（而不只是函数级）；`orchestration_acceptance.rs` 保持绿。
- [ ] F5.l 文档同步：03 §5（gate 的环境、沙箱、确认流程、串行）、09 §3 / §4 / §5（allowlist 来源、R6 在 `inplace` 模式下的归属规则与精度限制）、11 §6.3（gate 的子进程环境）、04 knowledge-pack（`gate_allowlist` 现在生效）。

### F6 子代理事件补全与审批（P1）

**问题与证据**【静态推演】

- `handleSubAgentChatEvent` 只处理 model / text / toolStart / toolEnd / fileChange / warning / error / done（`taskRunner.ts:390-451`），忽略 `thread`（CLI 会话 ID）、`approvalRequest`、`quota`、`reasoning`。敏感命令被拒后用户看不到审批卡片。
- CLI 若在被拒后仍发 `done`，任务会直接进 gate；若 CLI 退出时既无 `done` 也无 `error`（`run_cli` 在 `approval_requested` 时直接 `Ok(())`，`chat/mod.rs:983-985`），前端永远收不到终止信号，任务停在 `RUNNING`。
- 聊天里的审批续接机制（可复用）：`stores/chat.ts` 的 `resolveApproval`（`:542-620`）先 `resolveApproval` IPC（写 `approvals`，`allow_project` 还会写一条 `command_rules`，`lib.rs:1323-1366`），再把批准的命令并入 `sessionAllowed`，最后发一轮新的 turn：「User approved command: … Please execute it now.」或拒绝版本，沿用 `resume`。
- Rust 的 `UnifiedApprovalItem` / `UiEvent` 只是数据结构，应用层与前端都没有使用者；TODO P7.m6f 提到的 `collect_unified_approvals` 在源码里不存在。本专项不新增 `UiEvent` 通道，直接复用 `chat-event` 的 `approvalRequest`。

**目标契约**

```ts
// ActivePipelineSession / PipelineTaskInfo 新增（持久化见 F7）
threadIds: Record<string, string>;                       // taskId → CLI 会话 id（来自 thread 事件，每个 turn 覆盖为最新）
taskApprovals: Record<string, TaskApproval[]>;           // taskId → 审批项
taskAllowed: Record<string, string[]>;                   // taskId → 已批准的命令（作为 ChatRequest.sessionAllowed 续接）
interface TaskApproval { id: string; turnId: string; toolId: string | null; command: string; level: string; reason: string;
                         recoverable: boolean | null; resolved: boolean; decision?: "allow_once"|"allow_session"|"allow_project"|"deny"; }
```

- 状态流（S4）：运行中收到 `approvalRequest` → 记入 `taskApprovals`（任务仍 `RUNNING`，看板立即显示审批卡片）；本轮结束（收到 `done`，或 `chat-ended` 且既无 `done` 也无 `error`）时若存在未决审批 → 任务 `RUNNING → BLOCKED`（原因「等待审批：<命令>」），**不进入 gate**，释放并发槽位；用户决定后 `resolveApproval` IPC 记录并更新 `taskAllowed`，随后以 `resume = threadIds[taskId]`、`sessionAllowed = taskAllowed[taskId]`、提示词「User approved / denied command …」发新一轮（沿用该任务的 cwd、provider、model、role、`writeScope`、`AGENT` 超时），任务 `BLOCKED → READY → RUNNING`。若上一轮仍在运行，则决定只记录，等 `chat-ended` 后再续接。没有 thread id 时不能续接：UI 提示并允许「重新执行」。
- `quota` 事件：写任务日志（原文、重置时间、是否重试中）；熔断记录在 F15 接入。

**整改步骤**

- [ ] F6.a 事件补全：`handleSubAgentChatEvent` 处理 `thread`（写 `threadIds`，随流水线状态持久化）、`quota`（写日志）、`approvalRequest`（写 `taskApprovals`）；`reasoning` 折叠进任务日志（只记长度或首行，避免日志膨胀）。
- [ ] F6.b 终止信号：订阅 `chat-ended`（`onChatEnded`，F2.d）；轮次结束但既无 `done` 也无 `error` 的任务按上文处理（有未决审批 → `BLOCKED`「等待审批」；否则 `BLOCKED`「CLI 异常退出（无终止事件）」）。`cancelled == true` 的结束由 `cancelTask` 自己收尾，这里忽略。复现用例（前端）：合成 `approvalRequest` 后接 `chat-ended`（无 `done`）→ 任务 `BLOCKED` 且未调用 `executeGateCommand`；合成 `approvalRequest` 后接 `done` → 同样不进 gate。
- [ ] F6.c 审批界面与续接：看板里任务检视器与卡片显示审批项（复用 `ApprovalCard`，`PipelineView` 里按 `taskApprovals` 渲染）；决定后调用 `resolveApproval`（`sessionId` 用主会话 id，`turnId` / `toolId` 取审批项里的值），再按上文契约续接。复现用例：批准后 `startChatTurn` 被调用，参数含 `resume = threadIds[taskId]`、`sessionAllowed` 含该命令、`role = "EXECUTOR"`；拒绝后提示词是拒绝版本。
- [ ] F6.d Rust 侧用例：FakeAdapter 的 `Step::Denial` 后无 `done` 退出 → 事件序列是 `ApprovalRequest` → `ChatEnded`（`cancelled == false`），用来固定 F6.b 依赖的行为。验证：`cargo test -p tokage-native denial`。
- [ ] F6.e 验收：F6.a–F6.d 用例全绿；手动——流水线任务触发敏感命令 → 看板出现审批卡片，任务不被判 `DONE`、不占槽位；批准后任务续接并最终通过 gate。
- [ ] F6.f 文档同步：11 §4.3（拒绝后续接在流水线任务上的实现）、07 §5（审批汇入统一队列：现为按任务在看板展示，`UnifiedApprovalItem` 暂不接线，见 F17.d）、03 §8。

### F7 断点续跑（P1）

**问题与证据**【静态推演】

- `loadPipelineFromDisk` 用占位字段重建任务（`depends_on: []`、`goal: ""`、`acceptance` 与 `write_scope` 为空，`src/stores/execution.ts:367-380`），`contracts` 与 `upstreamArtifacts` 也没恢复（`execution.ts:410-422`）。原因是 `PipelineState`（`src-tauri/crates/tokage-planning/src/storage.rs:310`）只存 id / title / state / 模型 / turnId / 时间，不含完整任务、契约、上游产物、任务工作目录与 CLI 会话 ID。
- 之后 `progressDownstreamTasks` / `dispatchReadyTasks` 读的是 `pipeline.tasks`，会把所有 `PENDING` 任务当作依赖已满足而一次拉起，并且「未声明验收命令 → 直接通过」（`taskRunner.ts:514`）。
- `handleSubAgentChatEvent` 重建 ctx 时 `worktreeCwd` 取会话 cwd（`taskRunner.ts:370`），不是任务自己的目录，gate 会跑错地方。
- 恢复语义不一致：Rust `load_pipeline_state` 把 `RUNNING` / `VERIFYING` 置 `INTERRUPTED`（`storage.rs:348-390`，漏了 `REPAIRING`），TS 又做一遍；03 §10 写的是置 `READY` 并在原窗口续接；`lifecycle::recover_interrupted_tasks`（置 `READY`）没有调用方。`planning::PipelineStatus`（Running / Paused / Done / Interrupted）与 `orchestration::lifecycle::PipelineStatus`（Idle / Running / Paused / Cancelled / Completed / Blocked）同名异义。
- 现有测试只断言状态标志（`m5c: cold start recovery …`）。

**目标契约**（S5）

```rust
// tokage-planning/src/storage.rs；JSON 字段 camelCase。新增字段全部 #[serde(default)]，旧文件仍可读。
pub struct PipelineState {
    pub schema_version: u32,                           // 新写入为 2；缺省视为 1
    pub session_id: String,
    pub status: PipelineStatus,                        // RUNNING / PAUSED / DONE / INTERRUPTED
    pub plan_db_id: Option<i64>,                       // F12：plans.id
    pub base_cwd: Option<String>,                      // 会话工作目录
    pub integration: Option<IntegrationInfo>,          // F11：{ branch, root }
    pub tasks: Vec<PipelineTaskInfo>,
    pub task_items: Vec<TaskItem>,                     // 完整任务定义（v1 没有）
    pub contracts: Vec<PlanContract>,
    pub upstream_artifacts: HashMap<String, Value>,    // taskId → UpstreamArtifact（planning 不依赖 orchestration，用 Value 避免环）
    pub assignments: HashMap<String, Value>,           // taskId → TaskAssignment 快照（F1.f）
    pub deviation_sources: Vec<String>,                // F9：报告了契约偏离且未确认的任务
    pub task_states: HashMap<String, String>, pub task_logs: HashMap<String, Vec<String>>,
    pub gate_results: Vec<PipelineGateResult>, pub started_at: u64, pub updated_at: Option<u64>,
}
pub struct PipelineTaskInfo {                          // 在 v1 字段（id/title/state/assignedModel/turnId/startedAt/endedAt）之上新增：
    pub provider: Option<String>, pub thread_id: Option<String>,          // F6.a
    pub cwd: Option<String>,                                              // 任务实际工作目录
    pub worktree: Option<TaskWorktree>,                                   // F11：{ root, cwd, branch, baseCommit }
    pub repair_step: Option<String>,                                      // F13：RepairStep
    pub allowed_commands: Vec<String>, pub approvals: Vec<Value>,         // F6.c：taskAllowed / taskApprovals
    pub cost_usd: Option<f64>,                                            // F13.f
}
```

- TS 的 `PipelineState` / `PipelineTaskInfo`（`ipc.ts:2013-2041`）与 `ActivePipelineSession`（`execution.ts:14-28`）同步增加对应字段；附录 A.6 有 v1 / v2 的完整对照。
- 加载规则：v2 直接恢复；v1（`taskItems` 为空）按任务 id 用 `loadPlan(cwd, sessionId)` 的结果回填任务定义与契约，回填不了（计划已变化、任务 id 对不上）→ 标记「不可恢复」：看板显示「该流水线无法恢复（计划已变化），请重新生成」，只允许「丢弃」，禁止用占位对象继续调度。`load_pipeline_state` 把 `RUNNING` / `VERIFYING` / `REPAIRING` 的任务置 `INTERRUPTED`，流水线置 `INTERRUPTED`（Rust 做一次，TS 不再重复）。
- 「继续」动作（S5）：

```ts
export async function continueTask(sessionId: string, taskId: string): Promise<void>;
//  INTERRUPTED → READY → RUNNING。有 threadIds[taskId]：resume 该会话，提示词「继续未完成的任务；若上一轮已完成，请直接输出任务报告」；
//  没有：新窗口重跑（重新组装简报，附带已有改动清单）。cwd 用任务自己的工作目录（taskCwds[taskId]），目录不存在则回退 baseCwd 并在日志提示。
//  dispatchReadyTasks 永远不自动拉起 INTERRUPTED 任务。
```

**整改步骤**

- [ ] F7.a 复现用例（先红）：3 个任务（T2 依赖 T1，T3 依赖 T2），运行到 T1 `DONE`、T2 `RUNNING` → 序列化 → 清空 store → `loadPipelineFromDisk` → 断言任务的 `depends_on` / `goal` / `acceptance` / `write_scope` 完整、`contracts` 与 `upstreamArtifacts`（含 T1）恢复、T2 为 `INTERRUPTED`、T2 的 cwd 是它自己的目录；再 `dispatchReadyTasks`，断言不会拉起 T3，也不会自动拉起 T2。现状 T3 会被当作无依赖任务拉起。
- [ ] F7.b Rust schema v2：按契约扩展 `PipelineState` / `PipelineTaskInfo`；`load_pipeline_state` 增加 `REPAIRING → INTERRUPTED`；测试——v1 JSON 夹具（取自当前写出的格式）能加载且 `schema_version == 1`、v2 往返不丢字段、`REPAIRING` 被标记。验证：`cargo test -p tokage-planning pipeline`。
- [ ] F7.c 前端保存与恢复：`persistSessionImmediate` 写出 v2 全部字段；`loadPipelineFromDisk` 按加载规则恢复（含不可恢复分支）；`handleSubAgentChatEvent` 重建 ctx 时使用持久化的任务定义与任务 cwd（`taskRunner.ts:358-385`）；`upstreamArtifacts` 优先取持久化值，缺失时用 `getTaskReports`（现无调用方）从 SQLite 补。
- [ ] F7.d 「继续」动作：实现 `continueTask` 与看板上 `INTERRUPTED` 任务的「继续」按钮；复现用例——有 thread id 时 `startChatTurn` 带 `resume`，没有时是新窗口重跑，且都使用任务自己的 cwd。
- [ ] F7.e 清理重复与同名类型：删除 `loadPipelineFromDisk` 里重复的 `INTERRUPTED` 标记；`orchestration::lifecycle::PipelineStatus` 改名为 `PlanLifecycleStatus`；删除 `recover_interrupted_tasks` 及其测试 `test_recover_interrupted_tasks`（语义被 S5 取代）。
- [ ] F7.f 验收：F7.a / F7.b / F7.d 用例全绿；手动——执行中强退应用 → 重开 → 看板显示完整任务信息与日志 → 点「继续」后任务接着跑，下游依赖正确。
- [ ] F7.g 文档同步：03 §10（恢复语义改为 `INTERRUPTED` + 用户点「继续」）、08（`PipelineState` v2 字段）、07（执行窗口与 thread id）。

### F8 定义包更新入口收口（P1，D6 暂缓）

**问题与证据**【已对照源码】

- `OFFICIAL_MINISIGN_PUBKEY`（`src-tauri/src/knowledge.rs:113-114`）是 `minisign-verify` 库 README「Basic Example」里的示例公钥（3.4 的 E4）。它不是 Tokage 自己生成的密钥，官方更新通道现在签不出合法的包（失败即关闭，安全但等于不可用），TODO P8.m4a 对「签名校验」的描述不成立。
- 默认元数据地址 `https://raw.githubusercontent.com/iwpz/tokage/master/resources/knowledge/latest.json`（`src-tauri/src/lib.rs:941`）在仓库中不存在（`git ls-files | grep latest.json` 为空，内置包在 `src-tauri/resources/knowledge/builtin.json`）。
- `knowledgeAutoUpdate` 默认 `true`，`KnowledgeSettings.tsx:65-68` 在打开设置页时就会发起检查，即向 GitHub raw 发请求；这与 README「除你主动配置的定义包下载外，Tokage 不发起任何外网请求」相矛盾。`knowledge-pack-update-available` / `knowledge-pack-applied` 事件没有前端监听者。

**目标契约（过渡方案，D6 暂缓期间生效）**

```rust
// src-tauri/src/knowledge.rs
pub const OFFICIAL_MINISIGN_PUBKEY: Option<&str> = None;   // 暂缓：真实公钥待发布方案确定后填入；移除示例公钥
pub const ERR_UPDATE_KEY_UNSET: &str = "UPDATE_KEY_UNSET";
// check_and_stage_update：公钥为 None 且没有 override_pubkey 时，在任何网络请求之前返回 Err(ERR_UPDATE_KEY_UNSET)。
// lib.rs 的 check_knowledge_pack_update：同样提前返回；删除指向不存在文件的 default_url（metadata_url 必须由调用方提供）。
// BuildInfo（`build_info` 命令的返回，前端 `getBuildInfo`）新增字段 knowledge_update_enabled: bool（= OFFICIAL_MINISIGN_PUBKEY.is_some()），前端据此禁用入口。
```

- `knowledgeAutoUpdate` 默认值改为 `false`；现有安装通过 S10 的 `settingsVersion` 迁移强制置 `false`（迁移函数在 F1.g 已建好，这里只补用例）。
- `KnowledgeSettings.tsx`：`knowledge_update_enabled == false` 时，自动更新开关与「检查更新」按钮置灰并显示「官方更新通道尚未启用（未配置官方签名公钥）」，不发任何请求；用户自己配置的定义包下载（`fetchKnowledgePack`）不受影响。

**整改步骤**

- [ ] F8.a 防回归用例（先红）：Rust——① 公钥未配置时，对一个不可达的元数据地址（如 `http://127.0.0.1:1/`）调用 `check_and_stage_update`，返回的是 `UPDATE_KEY_UNSET` 而不是连接错误（证明没发请求）；② 源码里不得出现 README 示例公钥字符串（用 `include_str!` 读自身源文件断言）；③ 用测试密钥对 + `override_pubkey` 的正向路径——合法签名通过、改动一个字节失败、sha256 不符失败（复用 `knowledge.rs` 现有的校验函数测试）。前端——`knowledge_update_enabled == false` 时 `useKnowledge` 的检查函数不调用 IPC。
- [ ] F8.b 过渡实现：按契约改 `knowledge.rs` 与 `lib.rs`；`BuildInfo` 新增字段（Rust 与 `ipc.ts`）；`KnowledgeSettings.tsx` 与 i18n（中英文各一份）；`settings.ts` 的默认值与 S10 迁移里的 `knowledgeAutoUpdate`。
- [ ] F8.c 验收：默认构建下打开「设置 → 定义包」不产生任何网络请求；F8.a 用例全绿。
- [ ] F8.d 文档同步：04 §10（官方更新通道暂缓，D6）、README R11 段落（承诺现在成立）。

**暂缓项（D6，不在当前验收范围，仅记录接手所需信息）**

- 生成密钥对：`minisign -G -p tokage.pub -s tokage.key`；私钥放 GitHub Actions secret，公钥填入 `OFFICIAL_MINISIGN_PUBKEY`，指纹写进「关于」页。
- Release 工作流：对定义包生成签名 `minisign -S -s tokage.key -m pack.json`，把 `pack.json`、`pack.json.minisig`、`latest.json` 作为 GitHub Release 资产发布。
- 元数据契约：`RemotePackMetadata { pack_version, sha256, url, minisig }`（`knowledge.rs:94-101`），`minisig` 是 `.minisig` 文件的完整文本。校验顺序（`check_and_stage_update`）：拉元数据 → 版本相同则结束 → 下载包 → SHA-256 → minisign（`allow_legacy = false`）→ `parse_knowledge_pack` 且 `schema_version == 1` → 写 `<app data>/knowledge/pending.json`，发 `KnowledgePackUpdateAvailable`；用户确认后 `apply_staged_update` 单事务替换官方层。注意 `sha256` 来自同一份元数据，真实性只由签名保证。
- 默认元数据地址指向 Release 资产；`knowledgeAutoUpdate` 默认值与 README / 04 §10 的表述一并改回；前端补 `knowledge-pack-update-available` / `knowledge-pack-applied` 监听。

### F9 调度规则接入 Rust DagScheduler（P1，D1-A）

**问题与证据**【已读源码】

- 生产路径上的调度是 TS 的 `dispatchReadyTasks`，只做「依赖 + 全局并发数」，取候选按计划顺序截取（`taskRunner.ts:647-710`）；Rust `DagScheduler` 已实现的关键路径优先、按 Provider 限流、写入范围互斥、契约偏离暂停（`scheduler.rs`）在生产路径上都没生效。`schedule_plan_dag` 每次新建调度器，只能给出首批任务，没有调用方。
- Rust 调度器的占用口径只数 `Running`（`scheduler.rs:207-221`），而 TS 数 `RUNNING` + `VERIFYING`；`DagScheduler::new` 里 `plan_id` 是常量 `"plan-1"`（`scheduler.rs:132`）；`pick_next_dispatchable_tasks` 不产出 `DispatchIneligibleReason`（该枚举已定义但没人构造）。
- `DispatchIneligibleReason` 只有 `rename_all = "camelCase"`，struct 变体的内层字段仍是 snake_case（`conflicting_task_id`、`overlapping_paths`、`source_task_id`），而前端按 camelCase 读会得到 `undefined`（3.6 的 serde 坑）。
- 契约偏离暂停：`TaskReport.contract_deviations` 只被存进 SQLite（`taskRunner.ts:580`），没有任何地方据此暂停下游。

**目标契约**

```rust
// tokage-orchestration/src/scheduler.rs
impl DagScheduler {
    pub fn from_states(tasks: &[TaskItem], states: &HashMap<String, TaskState>,
                       assignments: &HashMap<String, (String /*provider*/, String /*model*/)>,
                       limits: ConcurrencyLimits, deviation_sources: &[String]) -> Self;
    pub fn pick(&self) -> PickResult;
}
pub struct PickResult {
    pub promoted: Vec<String>,                                    // 状态为 PENDING 且依赖全部 DONE 的任务（TS 先 PENDING→READY 再认领）
    pub dispatchable: Vec<String>,                                // 关键路径长度降序、id 升序；受全局 / Provider 并发、写入范围互斥、契约偏离暂停约束
    pub ineligible: HashMap<String, DispatchIneligibleReason>,    // READY（含 promoted）但本轮不能启动的任务及原因
}
// 占用口径（D1）：全局并发 = RUNNING + VERIFYING + REPAIRING；Provider 并发 = RUNNING + REPAIRING；写入范围占用 = RUNNING + VERIFYING + REPAIRING。
// 写入范围冲突用 F4 的 scope::scopes_overlap；契约偏离：deviation_sources 里任务的传递下游全部视为暂停。
// DispatchIneligibleReason 加 rename_all_fields = "camelCase"。

// IPC（替换 schedule_plan_dag；Provider 并发上限由命令内部从有效包的 providers.<id>.concurrency_hint 取，缺省 2）
pick_dispatchable_tasks(input: PickDispatchInput) -> PickDispatchOutput
```

```ts
interface PickDispatchInput {
  tasks: TaskItem[];                                   // 完整任务定义
  taskStates: Record<string, TaskState>;               // 前端 store 里的当前状态
  assignments: Record<string, [string, string]>;       // taskId → [provider, model]
  maxGlobalConcurrent: number;                         // = settings.maxParallel（至少 1）
  deviationSources: string[];
}
interface PickDispatchOutput { promoted: string[]; dispatchable: string[]; ineligible: Record<string, DispatchIneligibleReason>; }
type DispatchIneligibleReason =
  | { globalSlotsFull: { current: number; max: number } }
  | { providerSlotsFull: { provider: string; current: number; max: number } }
  | { writeScopeCollision: { conflictingTaskId: string; overlappingPaths: string[] } }
  | { contractDeviationPaused: { sourceTaskId: string } };
```

**整改步骤**

- [ ] F9.a 复现用例（先红）：Rust——① 关键路径：T1（路径长 3）排在 T3（路径长 2）前；② Provider 限流：同一 Provider 的 3 个 READY 任务、上限 2 → 2 个可派发、1 个 `providerSlotsFull`；③ 占用口径：`VERIFYING` / `REPAIRING` 任务占全局并发与写入范围；④ 写入范围：`src/**` 与 `src/server/**` 冲突 → `writeScopeCollision`（依赖 F4）；⑤ 契约偏离：来源任务的传递下游全部 `contractDeviationPaused`；⑥ `promoted`：依赖全部 `DONE` 的 `PENDING` 任务被晋升。验证：`cargo test -p tokage-orchestration scheduler`。前端——`vi.spyOn(ipc, "pickDispatchableTasks")` 返回脚本化输出，断言 `dispatchReadyTasks` 只启动 `dispatchable`、先应用 `promoted`、`opts.only` 只在 `dispatchable` 的交集里取。
- [ ] F9.b Rust 实现：`from_states` / `pick`、占用口径、`DispatchIneligibleReason` 属性；`DagScheduler` 去掉常量 `plan_id`（F12.d 之前先删字段，没人用）；删除 `schedule_plan_dag` 命令与 `ipc.ts` 封装；保留 `DagScheduler::new` 给既有测试用。
- [ ] F9.c IPC 与封装：`pick_dispatchable_tasks` 命令（`concurrency_hint` 来自有效包）与 `pickDispatchableTasks` 封装（非 Tauri 环境的 mock 与 `schedulePlanDag` 的 mock 同风格）。
- [ ] F9.d TS 接入：`dispatchReadyTasks` 构造输入（任务定义取 store 的 `tasks`，分配取 F1.f 的快照，`maxGlobalConcurrent` 取设置），调用 IPC，先对 `promoted` 做 `PENDING → READY`（日志「[就绪] 前置依赖已全部完成」），再认领 `dispatchable`；把 `ineligible` 存进会话状态（`taskWaitReasons`），`PipelineView` 在 `READY` 任务上显示等待原因（如「等待：写入范围与 T2 冲突」「等待：CLAUDE_CODE 并发已满 2/2」）。
- [ ] F9.e 契约偏离暂停：任务到 `DONE` 且报告的 `contractDeviations` 非空 → 把任务 id 加入 `deviationSources`（持久化，F7 schema 已预留）并给看板一条提示；看板横幅显示偏离内容与「确认继续」，确认后从 `deviationSources` 移除并重新派发。复现用例：偏离上游 → 下游不被派发；确认后被派发。
- [ ] F9.f 验收：F9.a / F9.e 用例全绿；手动——三个根任务（其中两个 write_scope 冲突、两个同 Provider）按规则分批启动，看板能看到等待原因。
- [ ] F9.g 文档同步：03 §3（占用口径、`pick_dispatchable_tasks` 契约、`promoted` / `ineligible` 语义）、09 §3。

### F10 流水线交互收尾（P2）

**问题与证据**【静态推演】

- 「终端接管」的命令是拼的 `claude --resume ${sessionId}-${task.id}`（`src/components/planning/PipelineView.tsx:219`），不是 CLI 真实会话 ID，且写死了 `claude`。Rust 的 `generate_terminal_resume_command`（`repair.rs:186-199`）是按字符串猜的：Cursor 写成 `cursor-agent resume`（实际是 `--resume`）、Copilot 写成 `copilot-cli resume`（实际可执行名 `copilot`，参数 `--resume`）、Antigravity 落到不存在的 `tokage-cli resume`，Aider / OpenCode 没有对应；也没有注册为 IPC。真实参数以各 Adapter 的 `launch` 为准（附录 A.15）。
- 「标记完成」与手动 gate 走 `handleSimulateStatus`（`PipelineView.tsx:162-184`）：只改状态，不触发下游调度；`handleRunGate`（`:186-215`）只跑第一条验收命令、超时 30 秒、工作目录用 `pipeline.cwd` 而不是任务目录，失败时置 `REPAIRING`（没有任何代码处理该状态）；`RUNNING` 任务上的「标记完成」（`:486-493`）与状态机冲突（`RUNNING → DONE` 不是合法转换）。
- 看板里 `localTaskStates` 与 store 的 `taskStates` 重复维护（`PipelineView.tsx:83-94,119-130`）。

**目标契约**

```rust
// tokage-native::chat::adapter::ProviderAdapter 新增
fn terminal_resume(&self, session_id: &str) -> Option<Vec<String>>;   // argv（不含 cd）；None = 该 CLI 不支持按 id 续接
// IPC：terminal_resume_command(provider: ProviderId, thread_id: Option<String>, cwd: String) -> Option<String>
//   返回可粘贴到终端的一行：cd '<cwd>' && <命令名> <参数…>（路径做 shell 单引号转义；命令名用注册表里的名字）
```

```ts
// 手动接管与自动流程共用同一个 gate 执行函数（从 processSubAgentCompletion 抽出）
async function runGatesForTask(ctx: GateRunContext): Promise<{ passed: boolean; evidence: string; results: PipelineGateResult[] }>;
```

- 接管状态流（S1）：任务 `BLOCKED` → 用户在终端接管并修改 → 看板「已完成接管，重跑关卡」→ `BLOCKED → VERIFYING` → `runGatesForTask`（任务自己的 cwd、全部验收命令、`confirmed` / `envAllow` / `run_id` 照常）→ 通过则 `VERIFYING → DONE`（日志「手动接管完成，验收通过」）并推进下游；失败则 `VERIFYING → BLOCKED`。无验收命令的任务重跑关卡视为通过。`RUNNING` 任务不再提供「标记完成」。

**整改步骤**

- [ ] F10.a 复现用例（先红）：Rust——对每个 provider 表驱动断言 `terminal_resume`（期望值见附录 A.15）；Aider 返回 `None`。前端——接管命令使用任务的真实 thread id 与 provider，没有 thread id 时返回 `null` 且 UI 显示「无可续接会话」；接管后重跑关卡的流程（`BLOCKED → VERIFYING → DONE`，通过后下游被派发）。
- [ ] F10.b Adapter 提供真实 resume 命令：实现 `terminal_resume`（7 个 Adapter）与 `terminal_resume_command` IPC；删除 `generate_terminal_resume_command` 的字符串猜测与 `tokage-cli` 兜底，`RepairDecision::MarkBlocked.terminal_resume_command` 改由它生成（F13 接线时使用）。
- [ ] F10.c 接管界面：`openTakeover` 调用 `terminal_resume_command`（带任务 cwd 与 thread id），删除 `PipelineView.tsx:219` 的拼接；弹窗按钮「已完成接管，重跑关卡」走上述状态流。
- [ ] F10.d 抽出共用 gate 函数并去掉模拟状态：`runGatesForTask` 供自动完成与接管共用；删除 `handleRunGate` 与 `handleSimulateStatus` 的生产用途（状态模拟如仍被测试需要，移入测试工具）；去掉 `RUNNING` 任务的「标记完成」。
- [ ] F10.e 看板状态源统一：删除 `localTaskStates` / `localSessionStatus` 的并行维护，看板只读 `useExecution`；`listGateRuns` 用 `planDbId`（F12.d 之后），之前先用 `String(sessionId)` 过渡，不再用常量 `"plan-1"`。
- [ ] F10.f 验收：F10.a 用例全绿；手动——`BLOCKED` 任务点「终端接管」，复制出的命令能在终端里真实续接（至少验证 Claude Code 与 Codex 各一次）；接管后点「重跑关卡」，通过则下游自动启动。
- [ ] F10.g 文档同步：03 §8（接管流程与各 CLI 的 resume 命令表）、11 §4.2（各 CLI 的会话续接参数）。

### F11 worktree 隔离完整实现（P2，D2 阶段二）

**问题与证据**【静态推演】：默认隔离改回 `inplace` 后（F1.g），`worktree` 模式仍是不完整的：每个任务从 `HEAD` 新建 worktree 且不含未提交改动（`git.rs:202,242`）；没有任何代码把任务分支合回去——`lifecycle::merge_task_worktree`（`lifecycle.rs:152-198`）未注册为命令、无调用方，并且它会对传入目录执行 `git checkout <target>`，**不能对用户主检出调用**；任务 worktree 与分支不会被清理（`removeSessionWorktree` 只在会话级操作里使用，`SessionActions.tsx`）；任务完成后也没人提交（CLI 的改动停留在未提交状态）。`git commit` / `git merge` / `git checkout` 在命令策略里都是 `sensitive`（`builtin.json` 的 `command_policy.sensitive`），不应让 CLI 去执行。

**目标契约**

- 命名：会话短 id `sid8` = 会话 id 前 8 位；集成分支 `tokage/<sid8>-integration`；任务分支 `tokage/<sid8>-<taskId>-<n>`（`n` 为该任务的尝试序号；`create_worktree` 只保留 `[A-Za-z0-9-]` 且最长 40 字符，`P0.M1a` 会变成 `P0M1a`，序号保证唯一）；worktree 目录在 `<app data>/worktrees/<repo>-<hash>/<name>`（沿用 `git.rs`）。
- `git::create_worktree(env, project, base, name, base_ref: Option<&str>)`：`base_ref` 缺省 `HEAD`；命令 `create_session_worktree(path, name, base_ref?)` 同步。
- 新增 IPC：`commit_task_worktree(root, message) -> bool`（`git add -A` 后用 `-c user.name=Tokage -c user.email=tokage@localhost commit`，避免用户未配置身份时失败；无改动返回 `false`）、`merge_task_worktree_cmd(integrationRoot, integrationBranch, taskBranch, task) -> WorktreeMergeOutcome`（调用 `lifecycle::merge_task_worktree`，且拒绝不在 `<app data>/worktrees/` 下的目录）、`prepare_integration_worktree(...)`（见冲突流程）、`remove_task_worktree(root, branch, keep_branch)`。`WorktreeMergeOutcome` 的 struct 变体字段要加 `rename_all_fields = "camelCase"`（现为 snake_case，前端会读到 `undefined`）。
- 合并串行：每个会话一把异步互斥锁，保证同一时刻只有一个 `merge_task_worktree` 在集成 worktree 里运行。
- 任务完成流程（worktree 模式）：gate 通过 → `commit_task_worktree` → 进入合并队列 → `merge_task_worktree_cmd`：成功 → `DONE`，任务 worktree 目录清理（分支保留到计划结束）；冲突（`Conflict { conflictedFiles, integrationTask }`，该函数已做 `merge --abort`）→ 原任务 `DONE`（它自己的 gate 已通过，日志写明「合并冲突，已生成集成任务」），并
  1. 把 `integrationTask`（`<id>-integration`，`depends_on = [原任务]`，`write_scope` = 冲突文件，验收命令沿用原任务）加入流水线；
  2. 原任务的所有下游任务追加依赖 `<id>-integration`；
  3. 集成任务的 worktree 从**集成分支**新建，再由 Tokage 执行 `git merge --no-commit --no-ff <原任务分支>`（预期冲突，忽略退出码）；CLI 只需在 `write_scope` 内改掉冲突标记，不执行任何 git 命令；gate 通过后 Tokage 提交合并、再把集成任务的分支合入集成分支（此时不会冲突）。
- 基线与 R6：每个任务记录 `baseCommit`（建 worktree 时集成分支的 HEAD，存入 `PipelineTaskInfo.worktree`）；R6 的权威改动集 = `git add -A` 后的 `git diff --cached --name-only <baseCommit>`，与 `fileChange` 事件集合取并集（`.tokage/` 在 `.git/info/exclude` 里，不会被加入）。
- 交付：计划全部 `DONE` 后，先在集成 worktree 里执行全量关卡（03 §9：优先用 RepoDigest 的构建 / 测试命令，F16 之前取所有任务验收命令的并集去重），再生成交付汇总（`generate_delivery_summary_cmd`），然后给出三选一：合并到当前分支（Tokage 在用户主检出里执行 `git merge --no-ff tokage/<sid8>-integration`，**仅当主检出工作区干净且经二次确认**，冲突则 `merge --abort` 并改为导出补丁）/ 导出补丁（`git diff <起点>..<集成分支>` 写成文件）/ 保留分支。默认不改动用户主检出。

**整改步骤**

- [ ] F11.a 复现用例（先红）：Rust（临时 git 仓库）——① 建集成分支与 worktree；② T1 的任务 worktree 从集成分支建，写文件并提交；③ 合入集成分支；④ T2 的任务 worktree 从集成分支建，断言能读到 T1 的文件；⑤ 构造冲突，断言返回 `Conflict` 且列出冲突文件、主检出与集成分支未被污染；⑥ `merge_task_worktree_cmd` 拒绝主检出路径。前端——派发时传 `baseRef`、gate 通过后依次调用 commit 与 merge。验证：`cargo test -p tokage-native git` 与 `cargo test -p tokage-orchestration lifecycle`。
- [ ] F11.b 实现 `create_worktree` 的 `base_ref`、上述四个 IPC 与 `WorktreeMergeOutcome` 的序列化属性；封装与 mock 同步。
- [ ] F11.c 会话集成分支：worktree 模式下开始执行时创建集成分支与集成 worktree，路径与分支名写入 `PipelineState.integration`（F7 schema 已预留）。
- [ ] F11.d 任务完成流程：commit → 合并队列 → `DONE` / 冲突分支；任务 worktree 路径与 `baseCommit` 写入 `PipelineTaskInfo.worktree`；下游任务的 worktree 从集成分支建（`dispatchSubAgentTask` 传 `baseRef`）。
- [ ] F11.e 冲突集成任务：按上文流程实现（依赖改写、集成 worktree 预合并、gate、提交、合入）；复现用例——两个任务改同一文件 → 第二个合并冲突 → 生成集成任务 → 集成任务通过 → 下游拿到合并后的代码。
- [ ] F11.f 交付动作：计划完成后的全量关卡、交付汇总、三选一界面与对应 IPC；复现用例——主检出有未提交改动时「合并到当前分支」被拒绝并提示导出补丁。
- [ ] F11.g 清理与启动核对：任务 / 会话结束后按 `DIRTY` 保护清理 worktree，分支在交付后清理；启动时对 `<app data>/worktrees/` 下的残留目录做 `git worktree prune` 并与流水线登记核对，孤儿目录在设置页列出让用户确认清理。
- [ ] F11.h worktree 模式的 R6：`inplace` 的归属排除规则（S8）在 worktree 模式下不需要；改用上文的权威改动集。复现用例——worktree 模式下两个并行任务各自越界一个文件，各自被正确归因。
- [ ] F11.i 验收与默认值切换条件：① F11.a–F11.h 用例全绿；② 真实 CLI 端到端——两个有依赖的任务加一个冲突场景在 `worktree` 模式下跑通，T2 的 gate 看得到 T1 产出；③ 执行结束前用户主检出的 `git status` 保持干净；④ 清理后无残留 worktree 与分支。全部满足后把 `isolation` 的默认值改为 `"worktree"`（只改默认值，不迁移已有安装——用户已有选择保持不变）并去掉设置页的「实验性」标注。
- [ ] F11.j 文档同步：03 §6（worktree 模式的完整流程、集成分支、冲突集成任务、交付动作）、03 §9、09 §4。

### F12 数据通路（P2）

**问题与证据**【已读源码与迁移】

- `usage_events` 的 `plan_id` / `task_id` / `domain` 在 `start_chat_turn` 落库处恒为 `NULL`，`equiv_cost_usd` 恒为 `NULL`（`lib.rs:1117-1139`）；06 §3 规定订阅制记入 `equiv_cost_usd`。
- `phase` 的写入值与 CHECK 约束不一致：`usage_events.phase` 只允许 `PLANNING` / `EXECUTION` / `REPAIR` / `CONTEXT` / `DIRECT`（V1 迁移），而 `start_chat_turn` 对 Reviewer 写 `"REVIEW"`、对 Tester 写 `"TEST"`（`lib.rs:1105-1116`），子代理的执行轮次被写成 `"DIRECT"`。违规写入整条失败，且 `let _ = storage.record_usage(row).await;` 把错误吞掉；一旦派发 `TESTER` / `REVIEWER` 角色的任务（F13.e），这些用量会静默丢失。
- `plans` / `plan_contracts` / `tasks` / `task_edges` 四张表在生产路径没有写入方（`insert_plan_record` 无调用方；唯一的 `INSERT INTO tasks` 在 `dashboard.rs` 的测试里）；`settle_plan_savings` 依赖它们（`dashboard.rs:549-660`）。
- 计划标识不统一：常量 `"plan-1"` 出现在 `scheduler.rs:132`、`lifecycle.rs:111`、`PipelineView.tsx:135`；`plans.id` / `tasks.id` / `usage_events.plan_id` 是 INTEGER，`gate_runs.plan_id` 与 `attempts.id` 是 TEXT。

**目标契约**

```ts
// ChatRequest 新增（Rust 同名 snake_case，#[serde(default)]）
planId?: number | null;     // plans.id
taskId?: number | null;     // tasks.id
domain?: string | null;
phase?: "PLANNING" | "EXECUTION" | "REPAIR" | "CONTEXT" | "DIRECT" | null;   // 缺省：Planner 或 plan 模式 → PLANNING，其余 → DIRECT；流水线派发显式传 EXECUTION，修复轮次传 REPAIR（S9）
attemptId?: string | null;  // usage_events.attempt_id；缺省沿用 "<session>|<provider>|<model>"，流水线派发传 turnId（F13.b 同时建 attempts 行）

// IPC：确认计划时写入关系表；幂等（同一 sessionId + plan.version 已存在则复用并返回已有 id）
record_plan_approved(input: RecordPlanApprovedInput): Promise<{ planId: number; taskIds: Record<string, number> }>
interface RecordPlanApprovedInput { sessionId: string; plan: Plan; assignments: TaskAssignment[]; estCostUsd: number; }
```

- `record_plan_approved` 在写线程里单事务写入：`plans`（`main_session_id = sessionId`、`version`、`parent_plan_id`、`goal = spec.goal`、`spec_json`、`est_cost_usd`、`status = 'RUNNING'`）、`plan_contracts`（`code` = 契约 id）、`tasks`（`code` = 任务 id，`contracts` / `write_scope` / `deliverables` / `acceptance` 存 JSON，`state` 取 `READY` / `PENDING`，`role` 由 domain 推导：`testing → TESTER`、`review → REVIEWER`、其余 `EXECUTOR`，`est_cost_usd` 取分配结果）、`task_edges`。主会话行必须已存在（外键）。同一事务里把该会话中 `plan_id IS NULL` 的 `PLANNING` 用量行（经 `attempts.main_session_id` 关联）回填为新 `plan_id`，规划成本因此计入计划（06 §5.2）。
- 成本口径：`equiv_cost_usd = (input·p_in + cache_read·p_in·r + cache_write·p_in·w + output·p_out) / 1e6`，`p_in` / `p_out` 取有效包候选的 `price_per_mtok`，`r` / `w` 取该模型 `cache_rates`（缺省 1.0）；候选按 `(provider, cli_model_arg 或 model_id)` 查找，查不到则 `None`。候选 `billing == "SUBSCRIPTION"` 时写 `equiv_cost_usd`、`cost_usd` 保持 CLI 给的值；`PAY_AS_YOU_GO` 且 CLI 没给 `cost_usd` 时，按同一公式写 `cost_usd`。
- 不做迁移：`tasks.state` 的 CHECK 只允许 9 个值（不含 `PAUSED_QUOTA` / `WAITING_UPSTREAM` / `INTERRUPTED`），本专项只在插入与计划结束时写 `state`，不镜像运行态；若以后要镜像这三个状态，先做迁移。

**整改步骤**

- [ ] F12.a 复现用例（先红）：Rust——① 对 Reviewer / Tester 角色的请求落库不失败，且 `phase` 落在允许的 5 个值内；② `record_plan_approved` 写入各表行数正确且幂等；③ 带 `plan_id` / `task_id` / `domain` 的请求，用量行带这些值；④ 订阅制模型的用量行 `equiv_cost_usd > 0` 且 `cost_usd` 为空，按量制模型得到 `cost_usd`；⑤ 确认计划后，该会话里此前 `plan_id` 为空的 `PLANNING` 用量行被回填为新 `plan_id`。验证：`cargo test -p tokage-storage` 与根 crate 的对应测试。
- [ ] F12.b `ChatRequest` 新字段（`planId` / `taskId` / `domain` / `phase` / `attemptId`）与 `phase` 推导；`equiv_cost_usd` 计算；`record_usage` 失败不再静默（`tracing::warn!` 带 turn id 与错误）；TS 的 `ChatRequest` 同步。
- [ ] F12.c `record_plan_approved`：Rust 命令与 `ipc.ts` 封装；`PlanReview.handleApprove` 在分配写回之后调用，把 `planId` / `taskIds` 存进流水线会话（`planDbId`，F7 schema）；规划用量的 `plan_id` 回填在命令内完成；派发子代理时把 `planId` / `taskId` / `domain` / `phase: "EXECUTION"` 填进 `ChatRequest`。
- [ ] F12.d 真实计划标识：`gate_runs.plan_id` 写 `String(planDbId)`；`generate_delivery_summary` 增加 `plan_id` 参数，去掉常量；`PipelineView` 的 `listGateRuns` 用 `planDbId`；`attempts.id` 约定为 turnId（F13.b）。
- [ ] F12.e 验收：F12.a 用例全绿；手动——确认一个计划并跑完，`plans` / `tasks` / `task_edges` / `plan_contracts` 有行，`usage_events` 里该计划的子代理轮次带 `plan_id` / `task_id`，订阅制轮次的 `equiv_cost_usd` 非空。
- [ ] F12.f 文档同步：08 schema（id 类型、`phase` 取值与角色的关系、`equiv_cost_usd` 口径、`tasks.state` 的取值限制）、06 §3。

### F13 执行质量闭环接线（P2，D7 的 A 组）

**问题与证据**【静态推演】

- gate 失败直接 `BLOCKED`（`taskRunner.ts:603-612`），03 §8 的三步修复梯级（`decide_repair_action_cmd`）没有调用方；`generate_implementation_fix_task_cmd`、`record_gate_run`、`upsert_attempt_cmd`、`derive_session_title_from_spec_cmd`、`check_plan_budget_overrun_cmd` 同样没有调用方。
- 角色写死为 `EXECUTOR`（`taskRunner.ts:338`）：`testing` / `review` 领域的任务也按执行者跑，03 §7 要求测试任务用 `TESTER`（写入范围不含实现目录、禁敏感命令），评审任务只读。
- 会话状态不会变：`MainSession.state` 只在创建时是 `"draft"`，之后没有任何代码更新它（`stores/projects.ts`），`main_sessions.status` 因此恒为 `IDLE`；`update_main_session_status_cmd` 直接写库，但会被下一次快照写入（`state_to_db`，`repo.rs:205-223`）覆盖，且表达不了 `PLANNING` / `PAUSED`。
- TS 的 `RepairDecision` 按 camelCase 读内层字段（`attemptId`、`excludeModels`、`terminalResumeCommand`），而 Rust 的 `RepairDecision`（`repair.rs:86-104`）只有 `rename_all = "camelCase"`，内层字段是 snake_case（3.6 的 serde 坑）——接线后会读到 `undefined`。`BlockedResolution::TerminalTakeover.resume_command` 同理。
- 没有「排除某些模型后重新分配单个任务」的函数；`reassign_unstarted_tasks` 只按 Provider 熔断重分配未开始的任务。`decide_repair_action_with_melted` 没有暴露为命令。
- 预算红线不完整：`check_plan_budget_overrun_cmd` 只有「超预估 50%」与「超预算上限」，09 §6 还有「修复成本占预估 30%」；设置项 `planBudgetUsd` / `pauseOnOverrun` 只在设置页里，没有消费者（附录 D）。
- `TaskReport` 没有「发现的缺陷」字段，测试任务发现的实现缺陷无法被解析并生成修复任务（03 §7）。

**目标契约**

```
gate 失败（或 R6 越界）
  → evidence = FailureEvidence { failedCommand, outputTail, outOfScopeFiles, previousChangedFiles }
  → decideRepairAction(task, repairSteps[taskId] ?? "INITIAL", attemptId = turnId, model, provider, evidence, isProviderMelted) → [nextStep, decision]
     tryInWindowRepair → VERIFYING → REPAIRING → RUNNING：同 cwd、同 provider / model、resume = threadIds[taskId]、phase "REPAIR"，
                         提示词 = 失败证据（FailureEvidence.to_markdown 的内容）；没有 thread id 时降级为 tryReassign 的新窗口做法
     tryReassign      → reassignTask(plan, taskId, excludeModels) 得到新 TaskAssignment → 更新 assigned_model 与 assignments 快照；
                         新窗口（新 turnId，不 resume），简报附带证据与上次改动清单；REPAIRING → RUNNING
     markBlocked      → BLOCKED(reason)；接管命令由 F10 的 terminal_resume_command 生成并展示
  → repairSteps[taskId] = nextStep（持久化，F7 schema）；每一步写 task_outcomes（F14.b）
```

- Rust：`RepairDecision`、`BlockedResolution` 加 `rename_all_fields = "camelCase"`（TS 类型不用改）；`decide_repair_action_cmd` 增加 `is_provider_melted: bool` 并在 `true` 时调用 `decide_repair_action_with_melted`；新增 `tokage-matching::assignment::reassign_task_excluding(plan, task_id, exclude_models, pack, learned, utils) -> Option<TaskAssignment>` 与 IPC `reassign_task(plan, taskId, excludeModels, …)`——排除后没有合格候选时返回错误，不静默落到 `candidates.first()`。
- 角色映射：`domain == "testing"` → `TESTER`，`"review"` → `REVIEWER`，其余 `EXECUTOR`；写入 `ChatRequest.role` 与 `tasks.role`（F12.c 同一套推导）。`REVIEWER` 只读（R7 在 `run_cli` 已有）。
- 缺陷回路：`TESTER` 任务的简报在「任务报告格式」段（**不动固定头部**，R10 要求头部逐字稳定）追加可选的「### 发现的缺陷」，每行 `- <实现任务 id>: <描述>`；`TaskReport` 增加 `defects: Option<String>`（`TaskReportRow` / TS 同步）。解析出缺陷时，对每个被点名的实现任务调用 `generate_implementation_fix_task_cmd` 生成 `<id>-fix` 任务并加入流水线（依赖该测试任务，`write_scope` 沿用实现任务）。修复后的复测由用户重跑测试任务，本专项不自动生成复测任务。
- 记录：`AttemptRow.id = turnId`，`ChatRequest` 的 `attemptId`（F12.b 已加）= turnId；派发时 `upsert_attempt_cmd`（`role`、`provider_name`、`model_id`、`workdir` = 任务 cwd、`status = "RUNNING"`、`predecessor_id` = 同任务上一次尝试），结束时更新 `status`（`DONE` / `FAILED` / `CANCELLED` / `TIMEOUT`）与 token 字段；`GateRunRow = { taskId: 任务 code, planId: String(planDbId), attemptId: turnId, command, exitCode, passed, durationMs, outputTail, errorMessage }`，每条 gate 命令执行后 `record_gate_run`。
- 会话状态：新增 `useProjects.setSessionState(sessionId, state)`，按流水线推进更新——计划等待确认 `review`、执行中 `running`、全部 `DONE` `done`、有 `BLOCKED` 且无运行中任务 `blocked`；`update_main_session_status_cmd` 删除（附录 C）。会话标题：计划确认后若标题仍是默认值，用 `derive_session_title_from_spec_cmd` 的结果。
- 预算（R12，09 §6）：新增 IPC `plan_cost_summary(planId) -> { planning, execution, repair, total }`（对 `usage_events` 按 `phase` 求 `SUM(COALESCE(cost_usd, equiv_cost_usd))`）；`check_plan_budget_overrun_cmd` 增加 `repair_cost_usd` 入参与「修复成本 > 预估 × 30%」判定；任务每次结束后检查：触及 `planBudgetUsd` 硬红线 → **一律**暂停队列；超预估 50% 或修复占比超 30% → 仅当 `pauseOnOverrun` 为真时暂停，否则只提示。暂停用 `pauseExecution` 并写明原因。

**整改步骤**

- [ ] F13.a 复现用例（先红）：前端（mock IPC）——① gate 失败 → `tryInWindowRepair` → 新 `startChatTurn` 带 `resume` 与证据，状态 `VERIFYING → REPAIRING → RUNNING`，通过后 `DONE` 且 `repairSteps` 记 `IN_WINDOW_REPAIR`；② 同窗口仍失败 → `tryReassign` → `reassignTask` 的新分配生效、新窗口无 `resume`、`assigned_model` 更新；③ 再失败 → `BLOCKED` 并带接管命令；④ R6 越界走同一梯级。Rust——`reassign_task_excluding` 排除当前模型后选成功概率最高的合格候选、候选耗尽返回错误；`RepairDecision` 序列化出的内层字段是 camelCase；预算判定三条红线。验证：`cargo test -p tokage-matching reassign`、`cargo test -p tokage-orchestration repair`。
- [ ] F13.b 记录：`upsert_attempt_cmd`、`record_gate_run`、`setSessionState`、会话标题；复现用例——跑完一个任务后 `attempts` 有行（id = turnId）、`gate_runs` 有每条命令的行、会话状态随流水线变化。
- [ ] F13.c 修复梯级（TS）与 Rust 属性修正：按契约实现；`repairSteps` 持久化；梯级的每一步日志写明「第 N 步：同窗口修复 / 改派 / 阻塞」。
- [ ] F13.d `reassign_task_excluding` 与 IPC `reassign_task`；封装与 mock。
- [ ] F13.e 角色映射与测试缺陷回路：按契约实现；复现用例——`testing` 领域任务以 `TESTER` 角色派发；测试报告列出缺陷 → 生成 `<id>-fix` 任务且依赖该测试任务；固定头部 `TASK_BRIEF_FIXED_HEADER` 未变（用快照测试锁住）。
- [ ] F13.f 预算红线：`plan_cost_summary` 与 `check_plan_budget_overrun_cmd` 扩展；设置项 `planBudgetUsd` / `pauseOnOverrun` 接线；复现用例——超硬红线暂停且忽略 `pauseOnOverrun`；超 50% 仅在 `pauseOnOverrun` 为真时暂停。
- [ ] F13.g 验收：F13.a–F13.f 用例全绿；手动（FakeAdapter 脚本化失败）——gate 失败后依次经过同窗口修复、改派、阻塞，看板日志可追踪每一步，成本进入预算统计。
- [ ] F13.h 文档同步：03 §7 / §8（缺陷回路、角色映射、梯级与 `is_provider_melted`）、09 §6、07（attempts 的 id 约定）。

### F14 学习、分层知识包与节省账本（P2，D7 的 B 组）

**问题与证据**【已读源码】

- **学习闭环在库层就没闭合**：`calculate_posterior_p` 与 `BetaAccumulator` 只在 `record_task_outcome` 里被用，分配（`assignment.rs`）只调用先验 `calculate_prior_p0`——写下去的学习数据没有读取方。`record_task_outcome`（`lib.rs:713-773`）本身也有错：按 `(model, domain, metric)` 取现值而忽略 `bucket` 与 `provider_name`，不同难度的统计互相串；缺省值是写死的 `4.8 / 1.2`（p0 = 0.8）而不是该模型 / 任务的先验；每次都从 `samples: 0` 起算，写回的 `samples` 恒为 1。
- **分层知识包只有 builtin 层生效**：`allocate_plan_models`、`assign_direct_task`、`reassign_unstarted_tasks_cmd` 都构造 `LayeredPacks { official: None, customs: vec![], overrides: vec![], learned_usage_profiles: None }`（`lib.rs:413-421,465-473,1726-1733`）；仪表盘的 `get_effective_pack`（`dashboard.rs:129-157`）读了 overrides 却没读 customs——四处构造互不一致。
- **自定义包与覆盖没有持久化**：`useKnowledge.customPacks`（多个自定义包）与 `modelOverrides` 只在内存里；`db.ts` 的 `capture()` 只持久化单个 `custom` 包，`set_custom_pack` 先 `DELETE FROM custom_packs` 再插一行（`repo.rs:441-455`，注释写着「Until P6.m3 there is at most one custom pack」）；`model_overrides` 表有存储函数却没有 IPC。重启后多包与覆盖全部丢失，Rust 侧也从来读不到。
- **手动分配的数值是假的**：`create_manual_assignment`（`assignment.rs:507-`）找不到匹配候选时静默落到 `pack.candidates.first()`（甚至硬编码 Claude），`est_cost_usd` 写死 `0.25`，`alternatives` 为空——用户手动改派后，计划的预估总成本不可信。
- **节省账本没有写入方**：`settle_plan_savings`（`dashboard.rs:549-660`）读 `tasks`（无写入方）与 `usage_events.cost_usd`（订阅制恒为空，所以「实际成本」被算成 0），`record_savings_event_cmd` 等无调用方；仪表盘的 `task_outcomes` / `savings_events` 因此恒空。
- 设置项 `confidenceThreshold` 没有消费者（附录 D）。

**目标契约**

- S7：`learned_stats` 的 `success_alpha` / `success_beta` 改存**观测权重累计**（从 0 起）：`FIRST_PASS` → α += 1.0；`FIXED_IN_WINDOW` → α += 0.5、β += 0.5；`REASSIGNED` / `BLOCKED` / `USER_TAKEOVER` → β += 1.0（口径即 `BetaAccumulator::update`）。键 `(provider_name, model_id, domain, bucket = "d<difficulty>", metric)`；每个结果写两行：Provider 级 `(provider, model, …)` 与模型级 `('*', model, …)`（`samples` 在读到的现值上加 1）。后验在分配时算：`p = calculate_posterior_p(prior_p0, κ = 6, α_obs, β_obs)`，优先用 Provider 级，样本数为 0 时退到模型级，再没有则只用先验。
- 结果写入点（F13 的修复梯级驱动）：任务首窗通过 → `FIRST_PASS`；同窗口修复后通过 → `FIXED_IN_WINDOW`；同窗口失败转改派 → 对**原模型**记 `REASSIGNED`，改派后新模型首窗通过记 `FIRST_PASS`；梯级耗尽 → `BLOCKED`；用户接管后通过 → `USER_TAKEOVER`。每条 `task_outcomes` 带 `p_predicted`（分配快照里的 `pSuccess`）、`est_cost_usd`、`actual_cost_usd`、`turns`。
- 用量画像与成本校准：任务结束时按 `usage_events` 求 `turns`、`calls_per_turn`、`growth_per_turn`（相邻轮 `context_tokens` 差的均值，≥ 0）、`output_per_turn`，经 `update_usage_profile_entry`（EMA α = 0.2，样本 < 5 与定义包初值按样本数加权混合）写 `learned_stats`（`bucket` = 任务规模 `S` / `M` / `L`）；`cost_calibration` = 实际 / 预估的 EMA，键 `(provider, model, '*', '*')`。
- 分配签名：`allocate_plan_models(plan, pack, learned: &LearnedIndex, initial_utils)`，其中 `LearnedIndex` 由 `storage.list_all_learned_stats()`（新增）构建，`HashMap<(provider, model, domain, bucket), (alpha_obs, beta_obs, samples)>`；`recompute_plan_allocation_with_overrides`、`allocate_plan_models_with_melted`、`reassign_unstarted_tasks`、`reassign_task_excluding`（F13.d）同样接收。`confidenceThreshold` 覆盖定义包 `success_model.p_min.default`，`difficulty_3` 取 `max(定义包值, 该阈值)`。
- 分层包：新增共享函数 `load_layers(db) -> LayeredPacks`——`builtin`；`official = None`（F8 暂缓，官方更新上线后接入）；`customs` 取 `custom_packs` 中 `enabled = 1` 的行按 `position` 排序、用 `parse_knowledge_pack` 解析（解析失败的包跳过并记 warn）；`overrides` 取 `model_overrides`；`learned_usage_profiles` 取 `learned_stats` 的用量画像指标。上述四处构造全部改为调用它。设置页里的「生效」视图是 `lib/knowledge.ts` 在 TS 里单独算的，F14 之后以 Rust 的结果为准，视图对齐不在本专项范围。
- 持久化：`custom_packs` 表本来就有 `position` / `enabled` / `name` / `pack_json` 列，改成逐包 upsert，不再「先清空再插一行」；`Snapshot` / `WriteBatch`（Rust 与 TS）携带 `customPacks` 数组，已有的单个 `custom_pack` 行迁移为 `position = 0`；新增 IPC `upsert_model_override` / `delete_model_override` / `list_model_overrides`，`useKnowledge` 在变更时调用、启动时加载。
- 节省结算：计划全部 `DONE`（或终态）后调用 `settle_plan_savings_cmd`；`settle_plan_savings` 先把每个任务的 `tasks.actual_cost_usd` 更新为其 `usage_events` 的 `SUM(COALESCE(cost_usd, equiv_cost_usd))`，`usage_cost` 同样用 `COALESCE`；规划成本因为 F12.c 的回填已带 `plan_id`，自然计入（06 §5.2：规划成本计入实际成本）。`DIRECT` 与 `CONTEXT` 两类节省事件随 F16 接入。

**整改步骤**

- [ ] F14.a 复现用例（先红，Rust）：① 记录 N 个 `FIRST_PASS` 后同一候选的 `p_success` 上升、记录 N 个 `BLOCKED` 后下降；② `d1` 与 `d3` 桶互不影响，Provider 级优先、模型级回退；③ `samples` 随记录累加；④ `create_manual_assignment` 对未知规格返回 `None`（不再落到 `candidates.first()`），`est_cost_usd` 来自成本模型而不是 `0.25`；⑤ 启用的自定义包与覆盖会改变分配结果；⑥ `custom_packs` 多行往返、`model_overrides` 往返。验证：`cargo test -p tokage-matching` 与 `cargo test -p tokage-storage`。
- [ ] F14.b `record_task_outcome` 重写：按契约改取现值的键、从 0 起累计、写两行、`samples` 累加；`fix_rate`（同窗口修复成功率 `q`）一并更新；`assignment.rs:393` 调用 `calculate_expected_cost` 时写死的 `q = 0.5` 改为读 `fix_rate`，没有样本时仍用 0.5。
- [ ] F14.c 用量画像与成本校准：按契约在任务结束时计算并写入；`learned_usage_profiles` 经 `load_layers` 进入 `merge_layers`，`calculate_c_exec` 使用。
- [ ] F14.d `load_layers(db)` 与四处调用点替换；分配命令改签名（含 TS 封装的参数）；`confidenceThreshold` 接入。
- [ ] F14.e 手动分配修正：`create_manual_assignment` 返回 `Option`，`None` 时命令返回明确错误、前端提示「找不到该模型」；`est_cost_usd` 用与自动分配相同的 `C_exec` 计算；填充 `alternatives`。
- [ ] F14.f 持久化：`customPacks` / `modelOverrides` 按契约落库与加载（含单包行迁移的迁移用例）。
- [ ] F14.g 学习与结算接线：F13 梯级的每个结果点调用 `record_task_outcome`；任务结束时写用量画像；计划完成时 `settle_plan_savings_cmd`（`referenceModel` 非 `auto` 时作为 `user_override` 传入），并让 `settle_plan_savings` 先回填 `tasks.actual_cost_usd`、用 `COALESCE` 求实际成本；设置页模型卡片的「先验 vs 实测」接上 `list_learned_stats` / `list_task_outcomes`（现在只渲染先验，`KnowledgeSettings.tsx:828` 的标题已写「vs 实测」）。复现用例——跑完 3 任务计划后 `task_outcomes`、`learned_stats`、`savings_events` 有行，且订阅制计划的「实际成本」不为 0。
- [ ] F14.h 验收：F14.a / F14.g 用例全绿；手动——跑完计划后仪表盘的首次通过率矩阵与「本周已省」非空并与明细对得上；再起一个同领域计划时，分配理由里能看到已学习的把握值。
- [ ] F14.i 文档同步：04 §4 / §9（学习统计口径与读取方式）、06 §5.2、04 knowledge-pack（`custom_packs` 多包与覆盖的持久化）。

### F15 Provider 健康与熔断（P2，D7 的 C 组）

**问题与证据**【已读源码】

- 熔断的 5 个命令（`trip_provider_melt_cmd`、`recover_provider_health_cmd`、`record_provider_failure_cmd`、`reset_provider_failures_cmd`、`list_provider_health_cmd`）、`reassign_unstarted_tasks_cmd`、`get_provider_utilization_now_cmd` 都没有调用方；`provider-melted` / `provider-recovered` 事件前端没有监听者（3.2）。
- `run_turn` 在 `RateLimited` / `AuthRequired` / `SpawnFailed` 时只往总线发 `ProviderMelted` / `ProviderDetected`，没有订阅者把它写进 `provider_health`，也不会转发给前端；超时、非零退出等失败没有任何事件（`chat/mod.rs:586-614`）。
- **状态所有权冲突**：provider 列表的快照持久化把检测状态写进 `provider_health.status`（`repo.rs:425-428`：`ON CONFLICT … SET status = ?3`），会覆盖熔断命令写入的 `MELTED`；`allocate_plan_models` 又靠 `status == "MELTED"` 排除 Provider（`lib.rs:423-432`）——下一次 provider 列表落库就让熔断失效。
- 熔断状态字符串：`MELTED`、`HALF_OPEN`、`READY`（恢复默认）、`UNKNOWN`（首次失败插入）；`MeltReason`：`QUOTA_SOFT` / `QUOTA_HARD` / `BUDGET` / `HEALTH` / `AUTH`（`health.rs`）。
- 设置项 `meltThreshold`（默认 0.9，06 §6.1 写的是 95%）、`activeQuotaQuery` 没有消费者；`query_provider_quota` 连前端封装都没有（附录 D）。

**目标契约**

- 总线新增 `DomainEvent::TurnFailed { provider, kind, message }`（`kind` 取 `TurnErrorKind`：`RateLimited` / `AuthRequired` / `Timeout` / `NonZeroExit` / `SpawnFailed` / `ParseError`）与 `DomainEvent::TurnSucceeded { provider }`，由 `run_turn` 在失败分类后与收到 `Done` 时发布；用户取消的轮次两者都不发。
- 应用层在 `setup` 里起一个订阅者：`Timeout` / `NonZeroExit` / `SpawnFailed` → `record_provider_failure`（连续 3 次 → `HEALTH` 熔断）；`RateLimited` → `trip_provider_melt(QUOTA_HARD, melted_until)`（`melted_until` 取 `Quota` 事件的 `reset_at`，未知则 1 小时后）；`AuthRequired` → `trip_provider_melt(AUTH)`；`TurnSucceeded` → `reset_provider_failures`（半开态下还要 `recover_provider_health`）。熔断与恢复都 `emit("provider-melted" | "provider-recovered", { provider, reason, until })`。
- `provider_health.status` 的所有权：快照写入不得覆盖 `MELTED` / `HALF_OPEN`——`ON CONFLICT … SET status = CASE WHEN provider_health.status IN ('MELTED','HALF_OPEN') THEN provider_health.status ELSE ?3 END`。
- 恢复（06 §6.3）：Rust 周期任务（启动后立即 + 每 60 秒）扫描 `melted_until` 已过的行——`QUOTA_*` 先取一次额度（有被动快照或 `activeQuotaQuery` 开启时），使用率 < 80% 才恢复，否则顺延 1 小时；`HEALTH` 熔断 10 分钟后置 `HALF_OPEN`；`HALF_OPEN` 的 Provider 在分配里可用但并发上限为 1（`pick_dispatchable_tasks` 命令内部读 `provider_health`），首个 `TurnSucceeded` → 恢复，首个失败 → 重新 `HEALTH` 熔断。
- 前端：`ipc.ts` 新增 `onProviderMelted` / `onProviderRecovered` 与 `queryProviderQuota(provider)` 封装；providers store 启动时 `list_provider_health_cmd` 并随事件合并；熔断的 Provider 在模型选择器里不可选（`runnable` 判定加入熔断态）、CLI 面板与顶栏显示原因与恢复时间，熔断徽章提供「手动恢复」按钮（`recover_provider_health_cmd`）。熔断由 Rust 订阅者负责，所以 `trip_provider_melt_cmd`、`record_provider_failure_cmd`、`reset_provider_failures_cmd` 三个命令不再需要，F17.c 删除。
- 设置：`meltThreshold` 默认值改为 0.95（与 06 §6.1 一致），作为 `QUOTA_SOFT` 阈值（使用率 ≥ 阈值熔断，< 80% 恢复）；`activeQuotaQuery` 开启后按使用率自适应轮询 `query_provider_quota`（使用率 < 50% 每 5 分钟、≥ 80% 每 2 分钟，应用空闲超过 10 分钟暂停）；`get_provider_utilization_now_cmd` 的结果作为 `allocatePlanModels` 的 `initialProviderUtils`。
- 熔断后的改派：收到熔断事件时，对该 Provider 上**未开始**的任务调用 `reassignUnstartedTasks(plan, currentAssignments, completedOrRunningIds, meltedProviders)` 并更新 `assigned_model` 与快照（日志「[改派] …」）；**运行中**的任务：`QUOTA_SOFT` 继续跑完，`QUOTA_HARD` / `AUTH` 导致轮次失败后，经 F13 的梯级以 `is_provider_melted = true`（TS 在失败后调用 `list_provider_health_cmd` 判定）直接进入第 2 步改派；改派不出可用 Provider 时任务置 `PAUSED_QUOTA`（原因写明 `melted_until`），`provider-recovered` 时回到 `READY`。

**整改步骤**

- [ ] F15.a 复现用例（先红，Rust）：① 连续 3 次 `TurnFailed(Timeout)` → `provider_health` 变 `MELTED` + `HEALTH`，`TurnSucceeded` 清零计数；② `RateLimited` → `MELTED` + `QUOTA_HARD`，`melted_until` 取自 `reset_at`；③ `AuthRequired` → `MELTED` + `AUTH`；④ provider 列表快照写入不覆盖 `MELTED` / `HALF_OPEN`；⑤ `melted_until` 到期的 `HEALTH` 行被置 `HALF_OPEN`，探测成功 → `READY`、失败 → 重新 `MELTED`；⑥ 用户取消的轮次不计失败。验证：`cargo test -p tokage-storage health` 与根 crate 订阅者测试。
- [ ] F15.b 状态所有权：改 `repo.rs:425-428` 的 SQL（见上），并补上述第 ④ 条用例。
- [ ] F15.c 总线事件与订阅者：`TurnFailed` / `TurnSucceeded` 的发布与应用层订阅者；`emit` 事件载荷。
- [ ] F15.d 恢复与半开：周期任务、`pick_dispatchable_tasks` 对 `HALF_OPEN` 的并发上限、`allocate_plan_models` 对 `HALF_OPEN` 的处理。
- [ ] F15.e 前端：事件监听、providers store 合并、模型选择器 / CLI 面板 / 顶栏展示（熔断徽章提供「手动恢复」，调用 `recover_provider_health_cmd`）；`meltThreshold` 默认值与 `activeQuotaQuery` 轮询；`queryProviderQuota` 封装与 CLI 设置页的「刷新额度」。
- [ ] F15.f 熔断改派：未开始任务自动改派、运行中任务经梯级改派、`PAUSED_QUOTA` 与恢复；复现用例——FakeAdapter 注入 429 后，未开始任务改派到可用 Provider，运行中任务经梯级第 2 步改派，无可用 Provider 时进入 `PAUSED_QUOTA` 并在恢复后继续。
- [ ] F15.g 验收：F15.a / F15.f 用例全绿，`orchestration_acceptance.rs` 的 `test_acceptance_fake_adapter_quota_exhaustion_and_auto_reassignment` 保持绿；手动——顶栏与 CLI 面板显示熔断原因与恢复时间。
- [ ] F15.h 文档同步：06 §6（订阅者、半开并发、`provider_health.status` 所有权、`meltThreshold` 默认值）、09 §2.4。

### F16 规划与上下文接线（P2）

**问题与证据**【已读源码】

- **项目摘要没有进入规划 prompt**：`get_repo_digest` / `render_planning_prompt` 无调用方（前端对 `digest` 零引用）；Plan 轮次的 prompt 由 `framed_prompt`（`chat/mod.rs:390-442`）直接包装，不含 RepoDigest。README 所说「强力模型只读项目摘要」没有发生。
- **计划格式规约有三份拷贝**：`tokage-native/src/chat/mod.rs` 的 `framed_prompt`（英文）、`tokage-planning/src/prompt.rs` 的 `PLAN_FORMAT_SPEC` / `PLANNING_SYSTEM_TEMPLATE`（中文，生产里没人用）、`usePlanSession.ts:295` 的 `repairPrompt`。
- 校验器拿不到项目已知命令：`validatePlan({ plan })` 不传 `allowed_commands`，`acceptance_auto_fill` 与 `unverified_acceptance_command` 永不触发（F5 的问题 7）。
- 重规划引擎没有接入：`PlanReview` 的「提出修改意见」只是把文字当聊天消息发出（`handleConfirmFeedback`，`PlanReview.tsx:172-179`），不走 `assemble_replan_prompt` / `compute_plan_diff` / `apply_plan_diff` / `generate_adaptation_tasks`；`assign_direct_task` 无调用方。
- 上下文引擎只接了「窗口内建议」：`Workspace.tsx:182` 调 `planInWindowContext`，`contextMinSaving` / `cacheAware` 没有传入；`apply_context_action_cmd`、`dismiss_context_action_cmd`、`build_compact_cmd`、`triage_main_session_message_cmd`、`evaluate_window_reuse_cmd` 与三个 context_actions 记录 / 结算命令无调用方。
- 设置项 `requireReview`、`planningAutoApproveBelowUsd`、`plannerModel` 没有消费者（附录 D）。

**目标契约**

- 规约单一来源 = `tokage-planning::prompt`。Plan 模式首轮的 prompt 由前端调用 `renderPlanningPrompt({ userRequest, repoDigest: digest.markdown, domains })` 生成（RepoDigest 来自 `getRepoDigest(cwd)`，已有 `DigestCache`）；`framed_prompt` 的 `(Role::Planner, true)` 分支删除长规约文本，只保留一行只读约束；格式修复提示改由新增 IPC `render_plan_repair_prompt(rawText)`（基于 `PLAN_FORMAT_SPEC`）生成，`repairPrompt` 常量删除。`tokage-planning` 依赖 `tokage-native`，所以拼装放在前端 / 应用层，不在 native 里调用 planning。
- `validatePlan` 调用传 `allowedCommands = digest.commands` 的去重并集（`ProjectCommands.all_commands()`）。
- 重规划流程：`PlanReview`「提出修改意见」→ `assembleReplanPrompt({ currentPlan, changeDescription, taskReports（来自 `getTaskReports` 转成 `TaskExecutionReport`）, affectedContractIds?, domains?, replanInputMaxTokens })` → 以 Plan 模式发一轮 → 解析 → `computePlanDiff(current, proposed)` → 新增 `PlanDiffView` 弹窗展示差异 → 用户确认 `applyPlanDiff`（版本 +1，旧版本归档到 `history/`）→ `generateAdaptationTasks` 为受影响的已完成任务生成适配任务 → `record_plan_approved` 写新版本（旧版本置 `SUPERSEDED`）。仅允许在流水线未启动或已暂停时重规划；同 id 任务沿用状态，删除的任务置 `CANCELLED`，新任务 `PENDING`。
- 直接分发：Auto 模式分流为 DIRECT 后，用 `assignDirectTask(domain, difficulty, size, initialProviderUtils)`（`domain` 取 `TriageResult.detectedDomains[0]`，缺省 `general`）推荐模型，经 `resolveTaskProviderAndModel` 映射成目录里的模型；该轮 `done` 后写 `savings_events`（`kind = "DIRECT"`，基线用 `calculate_task_counterfactual_cost` 对参考模型的估算）。
- 上下文：`planInWindowContext` 的参数里接入 `contextMinSaving`（→ `min_saving`）与 `cacheAware`（关闭时按无缓存估算）；建议条「采纳 / 忽略」调用 `record_context_action_cmd` + `apply_context_action_cmd` / `dismiss_context_action_cmd`，采纳压缩时用 `build_compact_cmd` 生成载荷作为下一轮输入，下一轮结束后 `settle_context_savings_cmd` 并写 `CONTEXT` 节省事件；主会话在流水线运行中追加消息时用 `triage_main_session_message_cmd` 分流（任务窗口 / 重规划 / 续接上次直接分发 / 直接分发）。**`evaluate_window_reuse_cmd`（流水线任务间窗口复用）暂不接线**：它依赖 worktree 隔离与简报复用策略，写入豁免表并注明理由。
- 设置：`requireReview` 为假、或计划预估总成本低于 `planningAutoApproveBelowUsd`（> 0）时，校验通过且无 Error 的计划在审阅界面自动确认（给出明确提示，可撤销前仍可暂停）；`plannerModel` 不是 `"auto"` 时 Plan 模式首轮用该模型，`"auto"` 保持现状。

**整改步骤**

- [ ] F16.a 复现用例（先红）：① Plan 模式首轮 `startChatTurn` 的 prompt 含 RepoDigest 的 Markdown 与规约（来自 `renderPlanningPrompt`），且不再含 `framed_prompt` 里的那份英文规约；② `validatePlan` 收到 `allowedCommands`；③ 重规划链路（mock IPC）——差异被展示、确认后版本 +1、适配任务生成；④ `planInWindowContext` 参数带 `minSaving` 与 `cacheAware`；⑤ 自动确认条件。
- [ ] F16.b RepoDigest 注入与规约去重：按契约改前端 Plan 轮次、`framed_prompt`、`usePlanSession.repairWithModel`；新增 `render_plan_repair_prompt`；`validatePlan` 传 `allowedCommands`。
- [ ] F16.c 重规划接线与 `PlanDiffView`。
- [ ] F16.d 直接分发推荐与 `DIRECT` 节省事件。
- [ ] F16.e 上下文接线：`planInWindowContext` 参数、建议条的采纳 / 忽略 / 压缩与结算、`CONTEXT` 节省事件、追加消息分流；`evaluate_window_reuse_cmd` 写入豁免表。
- [ ] F16.f 设置项接线：`requireReview`、`planningAutoApproveBelowUsd`、`plannerModel`。
- [ ] F16.g 验收：F16.a 用例全绿；手动——规划轮次的 prompt 里能看到项目摘要；修改意见走增量重规划并显示差异；建议条采纳压缩后下一轮发出压缩载荷并产生节省记录。
- [ ] F16.h 文档同步：02 §2 / §3 / §9（摘要注入、重规划流程）、05（建议条与结算）、10（`PlanDiffView`）。

### F17 杂项接线与豁免（P2）

**问题与证据**【脚本统计 + 已读源码】

- TODO P4.m2c 声称的侧边栏「外部会话」组件 `ExternalSessionsSection` 在源码里不存在，只有 i18n 文案（`externalSessions` / `noExternalSessions`）；`list_native_sessions` 无调用方。
- 保留策略（90 天日志 / 365 天用量 / `incremental_vacuum`）只有 `cleanup_retention_records` 的实现与一条测试，没有任何调度，`cleanup_retention_records_cmd` 无调用方。
- `get_builtin_knowledge_pack`（前端直接 `import builtin.json?raw`，`KnowledgeSettings.tsx:20`）、`classify_command`、`record_usage_event`、`list_usage_events`（用量已在 Rust 内部落库，仪表盘走 `get_usage_dashboard`）、`get_provider_proxy_status`（`ProxySettings` 用 `getAllProviderProxyStatuses`）、`update_main_session_status_cmd`（F13 已说明）、`list_savings_events_cmd`（仪表盘已聚合）与熔断的三个命令（F15 已说明）没有存在价值。
- 非命令类的库能力没有接线，脚本统计不到：`orchestration::tree` 的 `UiEvent` / `UiEventBatcher` / `SessionTreeNode` / `MainSessionStatus` / `UnifiedApprovalItem`（应用层与前端都没有使用者，TODO P7.m6c / m6f 所说的 50ms 合并与统一审批队列并不存在于运行路径）；`tokage-matching` 的 `select_planning_model` 与 `apply_user_preference_signal`（库函数，没有 IPC 与调用方；用户改派偏好信号在本文范围之外）；`tokage-context` 的纪律校验 `discipline.rs`。

**整改步骤**

- [ ] F17.a 外部会话侧边栏：在 `Sidebar.tsx` 新增 `ExternalSessionsSection`，用 `listNativeSessions` 展示（只读：供应商徽章、标题、更新时间；按项目或全局；30 秒刷新），i18n 文案已有。验证：组件测试（mock `listNativeSessions`）。
- [ ] F17.b 保留策略调度：Rust `setup` 里数据库打开后起一个低优先级任务——启动后 60 秒执行一次，之后每 24 小时一次，结果写 `tracing::info!`；设置页保留一个「立即清理」按钮调用 `cleanup_retention_records_cmd`。验证：Rust 用例——构造过期行，调度函数执行后被清理。
- [ ] F17.c 删除与豁免：删除附录 C 标注「删除」的 11 个命令及其 `ipc.ts` 封装（`schedule_plan_dag` 已在 F9.b、`update_main_session_status_cmd` 已在 F13.b 删除，其余 9 个是 `get_builtin_knowledge_pack`、`classify_command`、`record_usage_event`、`list_usage_events`、`get_provider_proxy_status`、`list_savings_events_cmd`、`trip_provider_melt_cmd`、`record_provider_failure_cmd`、`reset_provider_failures_cmd`）；暂缓的 `evaluate_window_reuse_cmd` 写入 `scripts/ipc-wiring-allowlist.txt` 并带理由。
- [ ] F17.d 非命令类库能力：`UiEvent` / `UiEventBatcher` / `SessionTreeNode` / `MainSessionStatus` / `UnifiedApprovalItem` 以及 `merge_task_worktree` 之外未被使用的库函数——逐项在代码注释与 TODO 里标注「库已实现，暂未接线」，或删除；不再在设计文档里宣称它们已生效。
- [ ] F17.e 验收：`scripts/check-ipc-wiring.py` 输出 0（或仅剩豁免表里带理由的命令）；CI 里该检查通过。
- [ ] F17.f 文档同步：01 §5（外部会话透视）、08（保留策略的调度）、07 §5（统一审批队列的现状）。

### F18 文档与实现对齐（收尾）

各专项的「文档同步」已经改掉了受影响的设计文档；这里处理总览性文档与残留的不一致。

- [ ] F18.a `README.md`：第 7 行状态说明改为与现状一致（现写「规划 / 匹配 / 编排闭环尚未实现」，与 TODO「P1–P8 全部完成」互相矛盾）；「开发与测试」一节已在 F0.e 加好。
- [ ] F18.b `TODO.md`：重新打开被本文推翻或只做了一半的「✓」并链接到对应 F 编号；「现状总览」表里的「完成」改成「库已实现 / 接线见 fix-todo」；`sub_agent_work_todo.md` 同样处理。
  - P4.m2c（外部会话侧栏组件缺失）→ F17.a；P5.m2a（摘要未注入规划 prompt）→ F16.b；P5.m6（重规划未接入 UI）→ F16.c；P6.m1d / m3（分层包只有 builtin 生效、多包与覆盖不持久化）→ F14.d / F14.f；P6.m5c（`assign_direct_task` 未调用）→ F16.d；P6.m6（学习闭环未闭合）→ F14.a–F14.g。
  - P7.m1b / m1c（调度规则未在生产路径生效）→ F9；P7.m3b / m3c（R5 被绕过、R6 未接线）→ F5；P7.m4（修复梯级未调用）→ F13；P7.m5a / m5c / m5d（交付汇总、恢复语义、worktree 合并）→ F11 / F7；P7.m6b / m6c / m6f / m6g（主会话状态、UiEvent 通道、统一审批、标题推导）→ F13.b / F6 / F17.d。
  - P8.m1（上下文引擎除窗口内建议外未接线）→ F16.e；P8.m2（熔断与预算未接线）→ F15 / F13.f；P8.m3a（节省账本无写入）→ F14.g；P8.m4a（官方公钥为示例 key）→ F8；P8.m5a（保留策略未调度）→ F17.b；P8.m6f（R5 / R6 等红线只有函数级测试，生产路径未覆盖）→ F5.k。
  - `sub_agent_work_todo.md`：m3d、m5b、m5c（真实 CLI 端到端没有对应测试，现有 E2E 用例全程替身）→ F1 / F3 / F7 / F10。
- [ ] F18.c `00-architecture-overview`：§6 的 `DomainEvent` 列表以代码为准（代码 7 种，文档列了 15 种；F2 / F15 新增的 `ChatEnded.cancelled`、`TurnFailed`、`TurnSucceeded` 一并写入）；§2 技术栈里的 shadcn/ui、TanStack Query、tree-sitter、tiktoken-rs 更正为实际实现（自研 `ui.tsx`、每文件前 300 行的行前缀符号提取、字节数 ÷ 4 估算 Token），或把换用列入后续计划；§5 的 crate 现状段落同步（7 个 crate，未创建 `tokage-usage` / `-session` / `-auth` / `-proxy`）；§7 命令表更新（新增、删除、废弃的命令）。
- [ ] F18.d `09-guardrails`：§7 末尾「现状」段落改为引用本文的验证结论——R5 / R6 / R16 的生产路径级测试在 F5.k 之后才成立。
- [ ] F18.e 把「完成的定义」（见 0 节）加入 `TODO.md` 的使用约定。
- [ ] F18.f 验收：README、TODO、00 总体架构与代码不再互相矛盾；`TODO.md` 里每个 ✓ 都能指到生产调用路径与测试。

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

`DomainEvent`（`tokage-core/src/bus.rs`）现有 7 种：`ProviderDetected { provider, status }`、`ProviderMelted { provider, reason: String }`、`ChatOutput { turn_id, event: EventKind }`、`ChatEnded { turn_id }`、`TriageDone { main_id, route }`、`TaskStateChanged { plan_id, task_id, old_state, new_state, reason }`、`KnowledgePackUpdateAvailable { pack_version, pending_path }`。变更：`ChatEnded` 增加 `cancelled: bool`（F2）；新增 `TurnFailed { provider, kind, message }`、`TurnSucceeded { provider }`（F15.c）。总线保证：每个订阅者按发布顺序收到全部事件，不丢弃；订阅者被丢弃后在下一次发布时移除。

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
- 定义包 `gate_allowlist`（`builtin.json`，14 条，`merge.rs` 只取 base 层）：`npm|pnpm|yarn|bun [run] test|build|lint|typecheck|check|format:check` + 仅 `--flag` 形式的参数；`npx tsc|vitest|jest|eslint|prettier --check|playwright test`；`cargo test|build|check|clippy|fmt --check`；`[python -m] pytest`；`ruff check|mypy|pyright`；`go test|build|vet`；`mvn|./mvnw test|verify|compile|package`；`gradle|./gradlew test|build|check|assemble`；`dotnet test|build`；`swift test|build`；`[bundle exec] rspec|rake test`；`phpunit`；`make test|build|check|lint|all`；`deno test|check|lint`。
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
| `isolation` | `"worktree" \| "inplace"` | `"inplace"`（原 `"worktree"`） | F11.i 验收后新安装默认改回 `"worktree"` | F1.g、F11 |
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
| `generate_delivery_summary_cmd` | `generateDeliverySummary` | 接线 | F11.f |
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
| `list_context_actions_cmd` | `listContextActions` | 接线（冷却判定需要历史） | F16.e |
| `settle_context_savings_cmd` | `settleContextSavings` | 接线 | F16.e |
| `trip_provider_melt_cmd` | `tripProviderMelt` | 删除（熔断由 Rust 订阅者执行） | F17.c |
| `record_provider_failure_cmd` | `recordProviderFailure` | 删除（同上） | F17.c |
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
| `evaluate_window_reuse_cmd` | `evaluateWindowReuse` | 豁免（依赖 worktree 隔离与简报复用策略，暂缓） | F16.e |
| `list_native_sessions` | `listNativeSessions` | 接线 | F17.a |
| `cleanup_retention_records_cmd` | `cleanupRetentionRecords` | 接线 | F17.b |
| `classify_command` | `classifyCommand` | 删除（设置页用 `authorizeCommand`） | F17.c |
| `get_builtin_knowledge_pack` | `getBuiltinKnowledgePack` | 删除（前端直接 `import builtin.json?raw`） | F17.c |

新增命令（接线后会出现，需要有调用方）：`pick_dispatchable_tasks`（F9）、`cancel_gate_command`（F5.e）、`terminal_resume_command`（F10.b）、`commit_task_worktree` / `merge_task_worktree_cmd` / `prepare_integration_worktree` / `remove_task_worktree`（F11.b）、`record_plan_approved`（F12.c）、`reassign_task`（F13.d）、`plan_cost_summary`（F13.f）、`upsert_model_override` / `delete_model_override` / `list_model_overrides`（F14.f）、`render_plan_repair_prompt`（F16.b）。

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
| `meltThreshold` | 0.9（改为 0.95） | 仅设置页 | F15.e |
| `activeQuotaQuery` | `false` | 仅设置页 | F15.e |
| `timeoutMultiplier` | 1 | `stores/chat.ts`、`taskRunner.ts`（传入 `ChatRequest`） | 已接线；`AGENT` 档位不乘它（F3） |
| `isolation` | `"worktree"`（改为 `"inplace"`） | `taskRunner.ts`、设置页 | F1.g、F11 |
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
| R11 | Tokage 只读取 CLI 配置的非密钥字段；Key / Token 的值读到即丢弃 | NativeEngine / 前端 | 成立；README「无其他外网请求」在 F8 之前被默认开启的自动更新破坏 |
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

## 基线记录（F0.a 填写）

环境：macOS 27.0 / arm64 · Node v22.16.0 · npm 11.4.2 · rustc/cargo 1.97.1 · 无 bubblewrap（macOS 用 sandbox-exec，不影响本机基线）

| 日期 | 命令 | 结果 | 失败 / 跳过项 |
|------|------|------|---------------|
| 2026-10-08 | `npm ci` | 通过（251 packages，约 5s） | — |
| 2026-10-08 | `npm run typecheck` | 通过（`tsc` exit 0） | — |
| 2026-10-08 | `npm test` | 通过：9 files / 56 tests，约 668ms（vitest 5.0.3） | — |
| 2026-10-08 | `cd src-tauri && cargo test --workspace --locked` | 通过：工作区合计 **352 passed / 0 failed / 3 ignored**（含根 crate `tokage` 7 个用例；首次编译约 84s） | 见下方 `#[ignore]` 清单 |

**图标风险复核**：`src-tauri/icons/` 已含 `32x32.png`、`128x128.png`、`128x128@2x.png`、`icon.ico`（及 `icon.icns` 等），根 crate 编译通过，无需 `npm run tauri icon`。

**`#[ignore]` 用例（均依赖本机真实 CLI / 探测，未跑）：**

| 用例 | 位置 | 说明 |
|------|------|------|
| `chat::live::direct_rm_is_not_run` | `tokage-native` `chat/mod.rs` | Direct 模式下 Codex/Cursor 不得擅自 `rm`；需本机 CLI |
| `chat::live::turn_per_cli` | 同上 | 对各已装 CLI 跑真实一轮（haiku / gpt-6-luna / auto 等） |
| `providers::live::detect_on_this_machine` | `tokage-native` `providers.rs` | 打印本机探测结果；手动验收用 |

手动跑忽略用例：`cd src-tauri && cargo test --lib chat::live -- --ignored --nocapture`（及 `providers::live`）。

**F0.d**：基线已落盘；`check-ipc-wiring.py` 已接入 `.github/workflows/ci.yml`；本地等价作业（typecheck / vitest / cargo workspace / ipc 脚本）全绿。本机 `git push` 因无 GitHub 凭据失败，远程 Actions 待使用者推送后确认变绿。
