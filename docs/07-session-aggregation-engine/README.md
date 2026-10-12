# 07 · Session Aggregation Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

1. Maintain tree structure: project → main session → plan → task → execution window → logs.
2. Push backend domain events to the frontend in real time.
3. Support human intervention: review and edit plan, reassign, pause/resume, append input, terminal takeover.
4. Attach CLI native sessions ([01 §8](../01-native-environment-engine/README.md)) to the tree read-only.

## 2. Tree Structure

```
Project
 └─ MainSession「User profile feature」
     ├─ Plan v1 (planning window: Claude / opus, read-only, $0.18)
     │    ├─ Spec / Contracts (.tokage/plans/12/)
     │    └─ Tasks
     │        ├─ T1 backend-api  ── Attempt #1  Codex / gpt-5.x      ✓ first pass
     │        ├─ T2 frontend-ui  ── Attempt #2  Cursor / composer     ✓ first pass
     │        ├─ T3 integration  ── Attempt #3  Codex (reuse #1 window)   ✓ 1 in-window fix
     │        └─ T4 testing      ── Attempt #4  Aider / deepseek-flash ✓
     ├─ Plan v2 (incremental: added T5)
     └─ Direct Task「Explain this error」 ── Attempt #5
```

- A task may have multiple execution windows (repair reassignment, resume); `attempts.predecessor_id` records continuation.
- **`attempts.id` convention (F13.b)**: Same as sub-agent `turnId` (`ChatRequest.attemptId`); on dispatch `upsert_attempt` (`status = RUNNING`), on end update to `DONE` / `FAILED` / `CANCELLED` / `TIMEOUT`. `gate_runs.attempt_id` is the same turnId.
- On window reuse, multiple tasks point at one CLI session, marked “reuse” on tree. Resume CLI session id from sub-agent `thread` event, persisted in pipeline state `threadIds` / per-task `threadId` (F7); user “Continue” passes as `startChatTurn.resume`.
- Planning window is itself an execution window, role `PLANNER`.

## 3. State

Main session (frontend `MainSession.state`, F13.b advanced via `setSessionState` with pipeline; no longer `update_main_session_status_cmd`):

| State | Meaning |
|------|------|
| `draft` | New / planning |
| `review` | Plan awaiting user confirmation |
| `running` | Tasks executing or queued |
| `blocked` | Task BLOCKED and no active task |
| `done` | Pipeline all terminal |

Task states: [03 §2](../03-orchestration-engine/README.md). Execution window: `STARTING | RUNNING | WAITING_INPUT | DONE | TIMEOUT | CANCELLED | ERROR`.

## 4. Event Push

Frontend establishes one `tauri::ipc::Channel` via `subscribe_events(channel)`:

```ts
type UiEvent =
  | { type: 'tree_changed'; mainId: number }
  | { type: 'plan'; planId: number; version: number; state: PlanState }
  | { type: 'task'; taskId: number; state: TaskState; attemptId?: number }
  | { type: 'assignment'; planId: number }                                  // allocation change (reassign, melt reallocation)
  | { type: 'attempt_status'; attemptId: number; status: AttemptStatus }
  | { type: 'log_append'; attemptId: number; logId: number; role: Role; delta: string }
  | { type: 'context'; attemptId: number; baseline: number; history: number; effectiveLimit: number; cacheWarmUntil?: string }
  | { type: 'context_suggestion'; attemptId: number; actionId: number; kind: ContextActionKind; estSavingPct: number; reason: string }
  | { type: 'gate'; taskId: number; passed: boolean; summary: string }
  | { type: 'cost'; planId: number; estimatedUsd: number; actualUsd: number }
  | { type: 'provider_status'; provider: ProviderId; status: ProviderStatus }
  | { type: 'notice'; level: 'info' | 'warn' | 'error'; message: string; action?: NoticeAction };
```

**Current state:** Main channel still `chat-event` / `chat-ended` / `provider-melted` and `execution` store. Minimal production wiring: `src/lib/uiEvents.ts` pushes `approval_required` on approval arrival (50ms batching, mirrors `UiEventBatcher`); full design table’s `log_append` / `set_focus` / reconnect backfill not wired end-to-end.

Flow control:

- Frontend `pushUiEvent` batches at 50ms (`uiEvents.ts`); Rust `UiEventBatcher` remains library capability.
- Frontend `set_focus(attempt_ids)` declares visible windows, push text deltas only for those; others status only (target contract, not wired).
- Write DB then push; on reconnect backfill via `get_chat_logs(attempt_id, after_log_id)` (target contract, not wired).

## 5. Human Intervention

| Action | Implementation |
|------|------|
| Review plan | Edit tasks, dependencies, write scope, contracts; `update_plan` then re-validate and allocate |
| Reassign | `reassign_task`, immediate for not-started; running must cancel first |
| Pause / resume queue | Running tasks finish current turn then stop scheduling |
| Append input | Send to selected execution window; or main session, triage decides destination ([05 §6](../05-context-lifecycle-engine/README.md)) |
| Handle blocked | Re-decompose (planning layer) / terminal takeover / skip |
| Approval (pipeline) | Sensitive command `approvalRequest` on board per task (`taskApprovals` + `ApprovalCard`); session-level unified approval queue at top of board (`collectUnifiedApprovals`); main chat approval still via session messages |
| Terminal takeover | Generate native resume command (e.g. `claude --resume <id>`) open in system terminal; after takeover user clicks “Done”, Tokage runs that task’s acceptance gate |
| Cancel | Process group SIGTERM, SIGKILL after 3 seconds |

## 6. Titles

Main session title defaults to first sentence of planning design draft “Goal”; direct dispatch uses first 40 characters of first message. User can rename.
