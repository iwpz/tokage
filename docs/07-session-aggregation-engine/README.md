# 07 · 聚合会话树引擎 (Session Aggregation Engine)

返回 [README](../../README.md)

## 1. 职责

1. 维护「项目 → 主会话 → 计划 → 任务 → 执行窗口 → 日志」的树形结构。
2. 把后端领域事件实时推送给前端。
3. 支持人工介入：审阅修改计划、改派、暂停恢复、追加输入、终端接管。
4. 把 CLI 原生会话（[01 §8](../01-native-environment-engine/README.md)）以只读方式挂到树上。

## 2. 树结构

```
Project
 └─ MainSession「用户资料功能」
     ├─ Plan v1（规划窗口：Claude / opus，只读，$0.18）
     │    ├─ Spec / Contracts（.tokage/plans/12/）
     │    └─ Tasks
     │        ├─ T1 backend-api  ── Attempt #1  Codex / gpt-5.x      ✓ 首次通过
     │        ├─ T2 frontend-ui  ── Attempt #2  Cursor / composer     ✓ 首次通过
     │        ├─ T3 integration  ── Attempt #3  Codex（续用 #1 窗口）   ✓ 同窗口修复 1 次
     │        └─ T4 testing      ── Attempt #4  Aider / deepseek-flash ✓
     ├─ Plan v2（增量：新增 T5）
     └─ Direct Task「解释这个报错」 ── Attempt #5
```

- 一个任务可以有多个执行窗口（修复改派、断点续跑）；`attempts.predecessor_id` 记录接续关系。
- **`attempts.id` 约定（F13.b）**：与子代理 `turnId`（`ChatRequest.attemptId`）相同；派发时 `upsert_attempt`（`status = RUNNING`），结束时更新为 `DONE` / `FAILED` / `CANCELLED` / `TIMEOUT`。`gate_runs.attempt_id` 同为该 turnId。
- 窗口复用时，多个任务指向同一个 CLI 会话，在树上标注「续用」。断点续跑的 CLI 会话 id 来自子代理 `thread` 事件，持久化在流水线状态的 `threadIds` / 每任务 `threadId`（F7）；用户点「继续」时作为 `startChatTurn.resume`。
- 规划窗口本身也是一个执行窗口，角色为 `PLANNER`。

## 3. 状态

主会话（前端 `MainSession.state`，F13.b 经 `setSessionState` 随流水线推进；不再写 `update_main_session_status_cmd`）：

| 状态 | 含义 |
|------|------|
| `draft` | 新建 / 规划中 |
| `review` | 计划待用户确认 |
| `running` | 有任务在执行或排队 |
| `blocked` | 有任务 BLOCKED 且无活跃任务 |
| `done` | 流水线全部终态 |

任务状态见 [03 §2](../03-orchestration-engine/README.md)。执行窗口：`STARTING | RUNNING | WAITING_INPUT | DONE | TIMEOUT | CANCELLED | ERROR`。

## 4. 事件推送

前端通过 `subscribe_events(channel)` 建立单个 `tauri::ipc::Channel`：

```ts
type UiEvent =
  | { type: 'tree_changed'; mainId: number }
  | { type: 'plan'; planId: number; version: number; state: PlanState }
  | { type: 'task'; taskId: number; state: TaskState; attemptId?: number }
  | { type: 'assignment'; planId: number }                                  // 分配变化（改派、熔断重分配）
  | { type: 'attempt_status'; attemptId: number; status: AttemptStatus }
  | { type: 'log_append'; attemptId: number; logId: number; role: Role; delta: string }
  | { type: 'context'; attemptId: number; baseline: number; history: number; effectiveLimit: number; cacheWarmUntil?: string }
  | { type: 'context_suggestion'; attemptId: number; actionId: number; kind: ContextActionKind; estSavingPct: number; reason: string }
  | { type: 'gate'; taskId: number; passed: boolean; summary: string }
  | { type: 'cost'; planId: number; estimatedUsd: number; actualUsd: number }
  | { type: 'provider_status'; provider: ProviderId; status: ProviderStatus }
  | { type: 'notice'; level: 'info' | 'warn' | 'error'; message: string; action?: NoticeAction };
```

流量控制：

- `log_append` 后端按 50ms 合并。
- 前端用 `set_focus(attempt_ids)` 声明可见窗口，只推送这些窗口的文本增量；其余只推状态。
- 先写库再推送；重连后用 `get_chat_logs(attempt_id, after_log_id)` 补齐。

## 5. 人工介入

| 操作 | 实现 |
|------|------|
| 审阅计划 | 编辑任务、依赖、写入范围、契约；`update_plan` 后重新校验与分配 |
| 改派 | `reassign_task`，未开始的任务立即生效；运行中的任务需先取消 |
| 暂停 / 恢复队列 | 运行中任务完成当前轮后停止调度 |
| 追加输入 | 选中执行窗口发送；或在主会话发送，由分流决定去向（[05 §6](../05-context-lifecycle-engine/README.md)） |
| 处理阻塞 | 重新拆解（交规划层）/ 终端接管 / 跳过 |
| 审批（流水线） | 敏感命令的 `approvalRequest` 按任务挂在看板（`taskApprovals` + `ApprovalCard`）；主聊天审批仍走会话消息。`UnifiedApprovalItem` 统一队列暂未接线（见 [fix-todo F17.d](../fix-todo.md)） |
| 终端接管 | 生成原生续接命令（如 `claude --resume <id>`）在系统终端打开；接管完成后用户点击「已完成」，Tokage 执行该任务的验收关卡 |
| 取消 | 进程组 SIGTERM，3 秒后 SIGKILL |

## 6. 标题

主会话标题默认取规划设计稿的「目标」首句；直接分发时取首条消息前 40 个字符。用户可改名。
