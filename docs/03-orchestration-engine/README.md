# 03 · 编排引擎 (Orchestration Engine)

返回 [README](../../README.md)

## 1. 职责

1. 把计划中的任务图转化为可执行的队列，按依赖与资源约束调度。
2. 在任务之间传递产物：上游的任务报告与改动成为下游的输入。
3. 每个任务完成后在本地执行验收关卡。
4. 在验收失败时执行修复兜底，并把结果反馈给匹配引擎学习。
5. 支持暂停、恢复、取消、重试，以及应用重启后的断点续跑。

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

审批阻塞（S4 / F6）：运行中收到 `approvalRequest` 时任务仍为 `RUNNING`（看板立即显示审批卡片）；本轮结束（`done` 或无终止事件的 `chat-ended`）且存在未决审批 → `RUNNING → BLOCKED`（「等待审批」），不进 gate；用户决定后 `BLOCKED → READY → RUNNING`，以 CLI `resume` 续接（见 [11 §4.3](../11-command-authorization/README.md)）。`chat-ended` 且既无 `done`/`error` 也无未决审批 → `BLOCKED`（「CLI 异常退出」）。

合法转换表见 [fix-todo F1.d / 附录 A.5](../fix-todo.md)。

## 3. 调度

```
loop on event (TaskStateChanged / ProviderRecovered / SlotFreed / PlanApproved):
    ready = tasks.filter(state == READY).sort_by(critical_path_length desc, id)
    for t in ready:
        if !slots.global_available(): break
        m = assignment[t]
        if !slots.provider_available(m.provider): try next t
        if scope_conflict(t, running_tasks): try next t
        if provider_melted(m.provider): Matching.reassign(t); try next t
        start(t)
```

- 关键路径优先：依赖链最长的任务先执行，缩短总时长。
- 并发上限：全局、单 Provider（[09 §3](../09-guardrails/README.md)）。
- 写入范围冲突：两个运行中任务的 `write_scope` 有交集时后者等待。规划校验已尽量避免这种情况。
- 熔断：任务启动前若其模型所在 Provider 已熔断，由匹配引擎重新分配；运行中的任务不迁移。
- 预算：计划预估成本在审阅时已确认；实际累计成本超出预估 50% 时暂停队列并提示（可配置）。

## 4. 任务的输入与产物

### 4.1 输入

任务启动时，上下文引擎组装任务简报（[05 task-brief](../05-context-lifecycle-engine/task-brief.md)）：

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
- 命令须匹配有效定义包的 `gate_allowlist`，或不含元字符且已写入 `acceptance.confirmedCommands`（计划审阅里可点「确认」）；含 `|` `>` `;` `&&` 等元字符的命令一律拒绝，不可确认（[09 §5](../09-guardrails/README.md)）。
- 子进程环境经 `assemble_gate_env` 剥离凭据与带 userinfo 的代理 URL；设置项 `gateEnvAllow` 可按名放行例外（[11 §6.3](../11-command-authorization/README.md)）。
- macOS / Linux 上经 `sandbox::gate_profile()` 的系统沙箱运行（写 worktree / tmp / 包管理缓存；不豁免各 CLI 自家目录；不限制出站网络）。无沙箱平台退化为未沙箱并在证据中提示。
- 用 `proc::isolate` + `Group` 回收整棵进程树；可用 `run_id` 取消（`cancelGateCommand`）。
- 输出流式读取，经 `OutputFilter` 脱敏后只保留尾部 200 行作为证据。
- 进 gate 前先做 R6：`changedFiles` 必须落在 `write_scope` 内（`inplace` 下排除与本任务时间重叠的其他任务的 scope 覆盖的文件）；越界 → 不跑 gate，任务 `BLOCKED`，可一键还原（[09 §4](../09-guardrails/README.md)）。
- `acceptance.criteria` 由执行模型在任务报告中逐条自查，不由 Tokage 判定。

全部通过 → `DONE`，记为「首次通过」（若在第一个执行窗口内完成）。

## 6. 工作区隔离

代码取值是 `inplace` | `worktree`（设置项 `isolation`）。历史上文档曾称 `inplace` 为 `shared`。

| 模式 | 说明 | 默认 |
|------|------|------|
| `inplace` | 所有任务在项目根目录执行，依靠写入范围不重叠保证并行安全 | 是（F1.g 止血；已有安装经 `settingsVersion` 迁移） |
| `worktree` | 每个并行任务在独立 `git worktree` 中执行，完成后由 Tokage 按依赖顺序合并回工作分支 | 实验性：完成 F11 前不会合并回主仓库；F11 验收通过后默认切到 `worktree` |

`worktree` 模式下合并冲突时，生成一个 `integration` 领域的修复任务交给匹配引擎分配。

## 7. 测试阶段

计划末尾的 `testing` 领域任务通常依赖全部实现任务：

- 由匹配引擎分配给在 `testing` 领域达标的低成本模型。
- 简报包含契约（测试的依据）与所有上游改动清单，不包含实现过程的对话。
- 关卡命令为项目的完整测试命令。

若测试任务发现实现缺陷（测试正确但实现不符合契约），测试模型在报告中列出缺陷，编排引擎为对应实现任务生成修复任务，而不是让测试模型去改实现代码（测试任务的写入范围不包含实现目录）。

## 8. 修复兜底

分发的目标是一次做对；修复只是安全网，其发生率（1 − 首次通过率）是匹配质量的核心指标。

```
关卡失败
  │
  ├─ 第 1 步：同一执行窗口内修复
  │     追加输入 = 失败证据（命令 + 输出尾部 + 越界文件）
  │     窗口缓存通常仍热，成本最低
  │
  ├─ 第 2 步：仍失败 → 匹配引擎重新分配
  │     排除当前模型，在该领域中选成功概率最高的模型
  │     新窗口，简报附带失败证据与上一次的改动清单
  │
  └─ 第 3 步：仍失败 → BLOCKED
        提供：交给规划层重新拆解此任务 / 用户在终端中接管 / 跳过
```

另：子代理触发敏感命令审批时也会进入 `BLOCKED`（「等待审批」），这与修复梯级无关；批准/拒绝后经 `resume` 续接同一 CLI 会话（[11 §4.3](../11-command-authorization/README.md)），通过后仍走上方 gate / 修复路径。

- 每一步的结果都写入 `task_outcomes`，匹配引擎据此更新该模型在该领域的成功概率。
- 每个任务的修复成本计入计划成本；计划的修复总成本超过预估成本的 30% 时暂停并提示。

## 9. 计划完成

所有任务 `DONE`（或被用户跳过）后：

1. 执行一次全量关卡（项目的完整构建与测试命令）。
2. 生成交付汇总：每个任务的模型、改动、验收结果、成本；计划总成本与预估对比；节省金额（[06 §5](../06-usage-melting-engine/README.md)）。
3. 可选：分配一个 `review` 领域任务，由评审模型只读地对照契约检查改动摘要（默认关闭，按计划规模建议）。

## 10. 断点续跑

- 任务状态、执行窗口 ID、报告全部持久化。
- 应用重启后：`RUNNING` 的任务因进程已被回收，置为 `READY` 并在原执行窗口上以「继续未完成的任务」续接（CLI 支持续接时），否则新窗口重跑。
- `PENDING` / `READY` 任务保持原分配，启动前重新检查 Provider 状态。
