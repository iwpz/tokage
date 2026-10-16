# 03 · 编排引擎 (Orchestration Engine)

[English](README.md) | **中文**

返回 [README](../../README.zh-CN.md)

## 1. 职责

1. 把计划中的任务图转化为可执行的队列，按依赖与资源约束调度。
2. 在任务之间传递产物：上游的任务报告与改动成为下游的输入。
3. 每个任务完成后在本地执行验收关卡。
4. 在验收失败时执行修复兜底，并把结果反馈给匹配引擎学习。
5. 支持暂停、恢复、取消、重试，以及应用重启后的断点续跑。

> m1 已完成代码与本机自动化验收；这不等于真实 CLI、桌面操作或跨进程崩溃验收。状态、持久化及恢复边界以 [m1g 验收记录](../reliability-m1g.md) 与所链 m1a–m1f 记录为准。

## 2. 任务状态

```
PENDING ──依赖全部完成──► READY ──获得执行槽──► RUNNING ──► VERIFYING
   ▲                                                       │
   │                                    通过 ◄─────────────┤
   │                                     │                 │ 失败
   │                                     ▼                 ▼
   │                                   DONE            REPAIRING ──► RUNNING
   │                                                       │
   └──── 计划更新后重置 ─────────── BLOCKED ◄───────────────┘ 兜底用尽 / 需用户决定
                                              │
                                              └──► VERIFYING（终端接管完成后重跑验收）
```

另有 `CANCELLED`、`SKIPPED`。取消语义（S2 / F3.d）：

- 用户对 `RUNNING` / `VERIFYING` 任务点「取消任务」→ `cancelTask`：`RUNNING` 时先 `await cancelChatTurn(turnId)`（进程组确认退出），再置 `CANCELLED`；`VERIFYING` 时先 `cancelGateCommand(runId)`（整组回收 gate 进程），再置 `CANCELLED`。
- 传递下游中仍为 `PENDING` / `READY` / `WAITING_UPSTREAM` 的任务置 `SKIPPED`（日志注明来源任务）。
- 迟到的 `done` / `error` 事件：若任务已不在 `RUNNING` / `VERIFYING`，只记日志，不再触发 gate / `DONE` / `BLOCKED`。

审批阻塞（S4 / F6）：运行中收到 `approvalRequest` 时任务仍为 `RUNNING`（看板立即显示审批卡片）；本轮结束（`done` 或无终止事件的 `chat-ended`）且存在未决审批 → `RUNNING → BLOCKED`（「等待审批」），不进 gate；用户决定后 `BLOCKED → READY → RUNNING`，以 CLI `resume` 续接（见 [11 §4.3](../11-command-authorization/README.zh-CN.md)）。`chat-ended` 且既无 `done`/`error` 也无未决审批 → `BLOCKED`（「CLI 异常退出」）。

合法转换表见 [fix-todo F1.d / 附录 A.5](../fix-todo.md)。

## 3. 调度

状态源在前端 store；每次派发由 TS 调用 IPC `pick_dispatchable_tasks`，Rust `DagScheduler::from_states` + `pick` 计算本轮结果（D1-A / F9）：

```
on event (TaskStateChanged / ProviderRecovered / SlotFreed / PlanApproved / DeviationConfirmed):
    out = pick_dispatchable_tasks({ tasks, taskStates, assignments, maxGlobalConcurrent, deviationSources })
    for id in out.promoted:           PENDING → READY
    for id in out.dispatchable:       claim READY → RUNNING and start
    store taskWaitReasons = out.ineligible   # 看板显示等待原因（不持久化）
```

`PickResult`：

| 字段 | 含义 |
|------|------|
| `promoted` | 状态为 `PENDING` 且依赖全部 `DONE` 的任务（TS 先晋升再认领） |
| `dispatchable` | 本轮可启动：关键路径长度降序、id 升序 |
| `ineligible` | `READY`（含刚晋升）但本轮不能启动的原因 |

占用口径（D1）：

| 槽位 | 计入状态 |
|------|----------|
| 全局并发 | `RUNNING` + `VERIFYING` + `REPAIRING` |
| 单 Provider 并发 | `RUNNING` + `REPAIRING` |
| 写入范围占用 | `RUNNING` + `VERIFYING` + `REPAIRING` |

`ineligible` 变体（camelCase）：`globalSlotsFull` / `providerSlotsFull` / `writeScopeCollision` / `contractDeviationPaused`。

- 关键路径优先：依赖链最长的任务先执行，缩短总时长。
- 并发上限：全局取设置 `maxParallel`；单 Provider 取定义包 `providers.<id>.concurrency_hint`（缺省 2）（[09 §3](../09-guardrails/README.zh-CN.md)）。
- 写入范围冲突：占用写入范围的任务与候选的 `write_scope` 有交集时后者等待（`scope::scopes_overlap`）。
- 契约偏离：任务 `DONE` 且 `contractDeviations` 非空（且不是「无」）→ 加入 `deviationSources`；其传递下游全部 `contractDeviationPaused`，看板横幅「确认继续」后移除并重新派发。
- 熔断：任务启动前若其模型所在 Provider 已熔断，由匹配引擎重新分配；运行中的任务不迁移。
- 预算：计划预估成本在审阅时已确认；实际累计成本超出预估 50% 时暂停队列并提示（可配置）。

## 4. 任务的输入与产物

### 4.1 输入

任务启动时，上下文引擎组装任务简报（[05 task-brief](../05-context-lifecycle-engine/task-brief.zh-CN.md)）：

- 任务卡（来自 `tasks.yaml`）。
- 引用的契约：给出 `.tokage/plans/<id>/contracts/` 下的路径，短契约同时内联。
- 上游任务报告：每个直接上游一份。
- 上游改动清单：上游任务实际修改的文件列表（来自 `git diff --name-only`）。

### 4.2 任务报告

任务完成时，执行模型按固定格式输出任务报告（≤ 300 Token），Tokage 再补充本地抽取的信息：

| 部分 | 来源 |
|------|------|
| 完成内容、偏离契约之处、给下游的提示 | 执行模型输出 |
| 修改的文件与行数 | `git diff --stat`（本地） |
| 验收结果 | 关卡执行结果（本地） |
| 用量与成本 | UsageEngine |

任务报告由执行模型在最后一轮输出中顺带写出，不单独发起请求。模型未按格式输出（超时、崩溃、遗漏）时，不为补报告追加一轮，改用本地抽取：改动文件、关卡结果、最近错误与模型最后一条输出的尾部，并在报告中标注「本地生成」。

偏离契约之处若非空，编排引擎暂停受影响的下游任务，提示用户确认，或交给规划层判断是否需要修改契约。

## 5. 验收关卡 (Gate)

任务的 `acceptance.commands` 由 Tokage 在本地执行，零 Token：

- 在任务的工作目录中依次执行，每条命令有独立超时（默认 300 秒）；全局串行（`Semaphore(1)`），排队时任务保持 `VERIFYING` 并写「排队等待验收」。
- 命令须匹配有效定义包的 `gate_allowlist`，或不含元字符且已写入 `acceptance.confirmedCommands`（计划审阅里可点「确认」）；含 `|` `>` `;` `&&` 等元字符的命令一律拒绝，不可确认（[09 §5](../09-guardrails/README.zh-CN.md)）。
- 子进程环境经 `assemble_gate_env` 剥离凭据与带 userinfo 的代理 URL；设置项 `gateEnvAllow` 可按名放行例外（[11 §6.3](../11-command-authorization/README.zh-CN.md)）。
- macOS / Linux 上经 `sandbox::gate_profile()` 的系统沙箱运行（写 worktree / tmp / 包管理缓存；不豁免各 CLI 自家目录；不限制出站网络）。无沙箱平台退化为未沙箱并在证据中提示。
- 用 `proc::isolate` + `Group` 回收整棵进程树；可用 `run_id` 取消（`cancelGateCommand`）。
- 输出流式读取，经 `OutputFilter` 脱敏后只保留尾部 200 行作为证据。
- gate 前后及提交前均核对磁盘与 index 快照，CLI 文件事件只作辅助；缺少基线、读取失败、越界或归属不明即阻塞，不自动修复。`inplace` 同一 checkout 串行执行，不排除其他任务 scope 内的改动（[09 §4](../09-guardrails/README.zh-CN.md)）。
- `acceptance.criteria` 由执行模型在任务报告中逐条自查，不由 Tokage 判定。

`DONE` 表示执行结束和产物保存。空命令可以结束任务，但不是自动验收通过；首次自动通过还要求真实 gate 通过、未修复且无多次尝试。每条命令保留 passed / failed / error / cancelled / not_run；取消后的迟到成功不改变结论。

## 6. 工作区隔离

代码取值是 `inplace` | `worktree`（设置项 `isolation`）。历史上文档曾称 `inplace` 为 `shared`。

| 模式 | 说明 | 默认 |
|------|------|------|
| `inplace` | 直接修改项目目录；同一 checkout 串行执行并保留执行前内容备份 | 可选 |
| `worktree` | 每任务独立 worktree；完成后 Tokage 提交并合入会话集成分支 | 是（F11.i；设计默认） |

### worktree 模式流程（F11）

命名：`sid8` = 会话 id 前 8 位字母数字；集成分支 `tokage/<sid8>-integration`；任务分支 `tokage/<sid8>-<taskId>-<n>`（`n` 为尝试序号）。目录在 `<app data>/worktrees/<repo>-<hash>/<name>`。

1. **会话开始**：首次派发前 `ensureSessionIntegration` 从 `HEAD` 建集成分支与集成 worktree，写入 `PipelineState.integration`。
2. **任务派发**：`create_session_worktree(..., baseRef = integration.branch)`；记录 `baseCommit`（集成 HEAD）到 `taskWorktrees`。
3. **任务交付**：先保存 `committing` 意图及验证快照，再执行验证提交，保存 `committed` / 固定 SHA 后合入集成分支；保存 `merged` 后才非强制清理任务 checkout，保留分支，最后保存 DONE。失败保留现场；继续/重试优先对账已有 Git 结果，不重跑模型。
4. **合并失败**：仅真实 unmerged 文件且 MERGE_HEAD 对应本次源提交才生成集成任务；锁、hook、权限、缺失分支等直接阻塞。resolver 使用可快照的私有冲突基线、继承模型并限制冲突文件；未解决标记禁止交付。准备身份和下游依赖落盘前原任务不能完成。详见 [m1d](../reliability-m1d.md)。
5. **交付**（计划全 `DONE` 后，见 §9）：在集成 worktree 跑全量关卡 → 交付汇总 → 用户三选一（合入当前分支 / 导出补丁 / 保留分支）。**默认不改用户主检出**。

## 7. 测试阶段

计划末尾的 `testing` 领域任务通常依赖全部实现任务：

- 角色映射（F13.e）：`domain == "testing"` → `TESTER`，`"review"` → `REVIEWER`，其余 `EXECUTOR`（写入 `ChatRequest.role` 与 `tasks.role`）。
- 由匹配引擎分配给在 `testing` 领域达标的低成本模型。
- 简报包含契约（测试的依据）与所有上游改动清单，不包含实现过程的对话；固定头部 `TASK_BRIEF_FIXED_HEADER` 不变（R10），测试任务在「任务报告格式」段追加可选的「### 发现的缺陷」。
- 关卡命令为项目的完整测试命令。

若测试任务发现实现缺陷（测试正确但实现不符合契约），测试模型在报告中列出缺陷（每行 `- <实现任务 id>: <描述>`），编排引擎调用 `generate_implementation_fix_task` 生成 `<id>-fix` 任务并加入流水线（依赖该测试任务，`write_scope` 沿用实现任务），而不是让测试模型去改实现代码。修复后的复测由用户重跑测试任务。

## 8. 修复兜底

分发的目标是一次做对；修复只是安全网，其发生率（1 − 首次通过率）是匹配质量的核心指标。

生产路径：普通 gate 失败 → `decide_repair_action`（可选 `is_provider_melted`：为真时跳过同窗口、直接改派）→ `repairSteps[taskId]` 持久化；每步日志写明「第 N 步：同窗口修复 / 改派 / 阻塞」。

```
普通关卡失败（范围与证据异常直接阻塞）
  │
  ├─ 第 1 步：同一执行窗口内修复（VERIFYING → REPAIRING → RUNNING）
  │     resume = threadIds[taskId]；phase = REPAIR；追加失败证据
  │     无 thread id 时降级为第 2 步
  │
  ├─ 第 2 步：仍失败 → `reassign_task`（排除当前模型）
  │     新窗口（不 resume），简报附带失败证据与上一次的改动清单
  │
  └─ 第 3 步：仍失败 → BLOCKED
        提供：交给规划层重新拆解此任务 / 用户在终端中接管 / 跳过
```

### 终端接管（S1 / F10）

`BLOCKED` 任务可在看板点「终端接管」：

1. IPC `terminal_resume_command(provider, threadId, cwd)` 生成可粘贴一行：`cd '<任务 cwd>' && <CLI> …`（各 Adapter 的 `terminal_resume`，见 [11 §4.2](../11-command-authorization/README.zh-CN.md) 与 fix-todo A.15）。无 thread id / Aider 等不支持按 id 续接时显示「无可续接会话」及提示。
2. 用户在外部终端改完后点「已完成接管，重跑关卡」→ `BLOCKED → VERIFYING` → 共用 `runGatesForTask`（任务自己的 cwd、全部验收命令）→ 通过则 `VERIFYING → DONE` 并推进下游；失败则回到 `BLOCKED`。无验收命令只结束执行，不记自动通过。
3. `RUNNING` 任务不再提供「标记完成」（非法 `RUNNING → DONE`）。

另：子代理触发敏感命令审批时也会进入 `BLOCKED`（「等待审批」），这与修复梯级无关；批准/拒绝后经 `resume` 续接同一 CLI 会话（[11 §4.3](../11-command-authorization/README.zh-CN.md)），通过后仍走上方 gate / 修复路径。

- 每一步的结果都写入 `task_outcomes`（F14），匹配引擎据此更新该模型在该领域的成功概率。
- 每个任务的修复成本计入计划成本；计划的修复总成本超过预估成本的 30% 时按预算红线处理（见 [09 §6](../09-guardrails/README.zh-CN.md)）。

## 9. 计划完成

计划执行结束后：

1. 在实际集成 worktree（或 inplace 工作目录）执行任务命令去重后的全量验收；保存所有命令结果。未配置、失败、取消、跳过任务均不能授权正常合并。
2. 生成正确会话/计划来源的报告汇总；缺少 gate 结果时摘要不声明成功。
3. 用户合并必须全部任务 DONE、ready、明确通过且证据完整；重新核对任务定义、状态、命令和 Git 内容，原生端再核对并按验收 SHA 合并。内容改变先重新验收；没有绕过失败的普通确认入口。
4. 合并成功保存 merged 和用户接受时间后才清理。导出补丁/保留分支继续保留原验收结论，不能替代通过。见 [m1f](../reliability-m1f.md)。

> 交付三选一 UI（看板）与孤儿 worktree 清理（设置 → 执行）见 F11.f / F11.g。

## 10. 断点续跑

- 流水线状态持久化到 `.tokage/execution/<sessionId>.json`（`PipelineState` schema v2：完整 `taskItems`、契约、上游产物、每任务 `threadId` / `cwd` / `allowedCommands` 等）。
- 应用重启后：`RUNNING` / `VERIFYING` / `REPAIRING` 的任务因进程已被回收，由 Rust `load_pipeline_state` 置为 `INTERRUPTED`，流水线状态亦为 `INTERRUPTED`；**不自动重新派发**。
- 用户在看板上点「继续」→ `continueTask`：有 CLI `threadId` 时以 `resume` 续接（提示词「继续未完成的任务；若上一轮已完成，请直接输出任务报告」）；没有则新窗口重跑并重新组装简报。cwd 必须匹配已有任务身份；worktree 元数据缺失即阻塞，不能回退为 inplace。存在 Git 交付意图先执行对账；未知中间状态保留现场。
- `PENDING` / `READY` 任务保持原分配与依赖；v1 旧文件若无法用当前计划回填任务定义 → 标记不可恢复，只允许丢弃。
