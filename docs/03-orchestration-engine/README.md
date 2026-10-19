# 03 · Orchestration Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

Reliability acceptance (2026-10-19): [m2g](../reliability-m2g.md) distinguishes layered faults from full desktop SIGKILL. Uncertain CLI outcomes block after recovery and preserve their checkout; recovery does not automatically redispatch or deliver, and missing billing remains unknown. See the acceptance matrix for actual scope and untested cases.

## 1. Responsibilities

1. Turn the plan’s task graph into an executable queue; schedule by dependencies and resource constraints.
2. Pass artifacts between tasks: upstream task reports and changes become downstream input.
3. Run acceptance gates locally after each task completes.
4. On gate failure, run repair fallback and feed results to the matching engine for learning.
5. Support pause, resume, cancel, retry, and resume-after-restart from checkpoint.

> m1 has passed code and local automation acceptance. Real CLI, desktop interactions and cross-process crash acceptance remain open. See [m1g acceptance](../reliability-m1g.md) and linked m1a–m1f records for state, persistence and recovery boundaries.

## 2. Task States

```
PENDING ──all deps done──► READY ──slot acquired──► RUNNING ──► VERIFYING
   ▲                                                       │
   │                                    pass ◄─────────────┤
   │                                     │                 │ fail
   │                                     ▼                 ▼
   │                                   DONE            REPAIRING ──► RUNNING
   │                                                       │
   └──── reset after plan update ─── BLOCKED ◄──────────────┘ fallback exhausted / user decision
                                              │
                                              └──► VERIFYING (re-run gate after terminal takeover)
```

Also `CANCELLED`, `SKIPPED`. Cancel semantics (S2 / F3.d):

- User clicks “Cancel task” on `RUNNING` / `VERIFYING` → `cancelTask`: for `RUNNING` first `await cancelChatTurn(turnId)` (process group confirmed exited), then `CANCELLED`; for `VERIFYING` first `cancelGateCommand(runId)` (whole gate group reaped), then `CANCELLED`.
- Downstream still `PENDING` / `READY` / `WAITING_UPSTREAM` → `SKIPPED` (log cites source task).
- Late `done` / `error` events: if task no longer `RUNNING` / `VERIFYING`, log only; do not trigger gate / `DONE` / `BLOCKED`.

Approval block (S4 / F6): on `approvalRequest` while running, task stays `RUNNING` (board shows approval card immediately); when turn ends (`done` or `chat-ended` without terminal event) and pending approval exists → `RUNNING → BLOCKED` (“Waiting for approval”), no gate; after user decision `BLOCKED → READY → RUNNING`, resume via CLI `resume` (see [11 §4.3](../11-command-authorization/README.md)). `chat-ended` with neither `done`/`error` nor pending approval → `BLOCKED` (“CLI exited abnormally”).

Legal transitions: [fix-todo F1.d / Appendix A.5](../fix-todo.md).

## 3. Scheduling

State lives in frontend store; each dispatch TS calls IPC `pick_dispatchable_tasks`, Rust `DagScheduler::from_states` + `pick` computes this round (D1-A / F9):

```
on event (TaskStateChanged / ProviderRecovered / SlotFreed / PlanApproved / DeviationConfirmed):
    out = pick_dispatchable_tasks({ tasks, taskStates, assignments, maxGlobalConcurrent, deviationSources })
    for id in out.promoted:           PENDING → READY
    for id in out.dispatchable:       claim READY → RUNNING and start
    store taskWaitReasons = out.ineligible   # board shows wait reason (not persisted)
```

`PickResult`:

| Field | Meaning |
|------|------|
| `promoted` | Tasks in `PENDING` with all deps `DONE` (TS promotes then claims) |
| `dispatchable` | Start this round: descending critical path length, ascending id |
| `ineligible` | `READY` (incl. just promoted) but cannot start this round and why |

Slot accounting (D1):

| Slot | Counted states |
|------|----------|
| Global concurrency | `RUNNING` + `VERIFYING` + `REPAIRING` |
| Per-Provider concurrency | `RUNNING` + `REPAIRING` |
| Write scope occupancy | `RUNNING` + `VERIFYING` + `REPAIRING` |

`ineligible` variants (camelCase): `globalSlotsFull` / `providerSlotsFull` / `writeScopeCollision` / `contractDeviationPaused`.

- Critical path first: longest dependency chain starts earlier to shorten total time.
- Concurrency caps: global from setting `maxParallel`; per Provider from definition pack `providers.<id>.concurrency_hint` (default 2) ([09 §3](../09-guardrails/README.md)).
- Write scope conflict: candidate waits if its `write_scope` intersects scopes held by occupying tasks (`scope::scopes_overlap`).
- Contract deviation: task `DONE` with non-empty `contractDeviations` (and not “none”) → add to `deviationSources`; all transitive downstream `contractDeviationPaused`, board banner “Confirm continue”, then remove and re-dispatch.
- Melting: before start, if task’s model Provider is melted, matching re-assigns; running tasks are not migrated.
- Budget: plan estimate confirmed at review; when actual cumulative cost exceeds estimate by 50%, pause queue and prompt (configurable).

## 4. Task Input and Artifacts

### 4.1 Input

On task start, context engine assembles task brief ([05 task-brief](../05-context-lifecycle-engine/task-brief.md)):

- Task card (from `tasks.yaml`).
- Referenced contracts: paths under `.tokage/plans/<id>/contracts/`; short contracts also inlined.
- Upstream task reports: one per direct upstream.
- Upstream change list: files actually modified by upstream (`git diff --name-only`).

### 4.2 Task Report

On completion, executor outputs a task report in fixed format (≤ 300 tokens); Tokage adds locally extracted info:

| Section | Source |
|------|------|
| What was done, contract deviations, hints for downstream | Executor output |
| Files changed and line counts | `git diff --stat` (local) |
| Acceptance result | Gate results (local) |
| Usage and cost | UsageEngine |

Report is written in the executor’s final turn output, not a separate request. If model does not follow format (timeout, crash, omission), no extra turn to fix report; use local extraction: changed files, gate result, recent errors, tail of last model output, mark report “locally generated”.

If contract deviations non-empty, orchestration pauses affected downstream, prompts user to confirm, or escalates to planning to revise contracts.

## 5. Acceptance Gate

Task `acceptance.commands` run locally by Tokage, zero token:

- Run sequentially in task working directory; each command has its own timeout (default 300s); globally serial (`Semaphore(1)`); while queued task stays `VERIFYING` with “Queued for acceptance”.
- Commands must match effective definition pack `gate_allowlist`, or have no metacharacters and be in `acceptance.confirmedCommands` (user can “Confirm” in plan review); commands with `|`, `>`, `;`, `&&`, etc. always rejected, cannot confirm ([09 §5](../09-guardrails/README.md)).
- Child env via `assemble_gate_env` strips credentials and proxy URLs with userinfo; setting `gateEnvAllow` can allow named exceptions ([11 §6.3](../11-command-authorization/README.md)).
- On macOS / Linux run under system sandbox from `sandbox::gate_profile()` (write worktree / tmp / package cache; no exemption for CLI home dirs; outbound network not restricted). Platforms without sandbox run unsandboxed with notice in evidence.
- `proc::isolate` + `Group` reaps whole tree; cancel via `run_id` (`cancelGateCommand`).
- Output streamed, redacted via `OutputFilter`, keep last 200 lines as evidence.
- Validate disk and index snapshots before/after gates and before commit; CLI file events are supplementary. Missing evidence, read failures, scope violations or ambiguous ownership block without automatic repair. Tasks sharing an inplace checkout run serially; other tasks’ scopes never exclude observed edits ([09 §4](../09-guardrails/README.md)).
- `acceptance.criteria` self-checked by executor in task report; Tokage does not judge.

`DONE` means execution ended and artifacts were saved. Empty commands may finish a task but do not pass automatic acceptance. First automatic pass also requires real passing gates, no repair and no multiple attempts. Each command retains passed / failed / error / cancelled / not_run; late success after cancellation cannot change the verdict.

## 6. Workspace Isolation

Code values: `inplace` | `worktree` (setting `isolation`). Older docs called `inplace` `shared`.

| Mode | Description | Default |
|------|------|------|
| `inplace` | Edits project directly; shared checkout runs serially with pre-turn content backups | Optional |
| `worktree` | Per-task worktree; Tokage commits and merges into session integration branch | Yes (F11.i; design default) |

### worktree flow (F11)

Naming: `sid8` = first 8 alphanumeric chars of session id; integration branch `tokage/<sid8>-integration`; task branch `tokage/<sid8>-<taskId>-<n>` (`n` = attempt number). Directories under `<app data>/worktrees/<repo>-<hash>/<name>`.

1. **Session start**: before first dispatch `ensureSessionIntegration` creates integration branch and worktree from `HEAD`, writes `PipelineState.integration`.
2. **Task dispatch**: `create_session_worktree(..., baseRef = integration.branch)`; record `baseCommit` (integration HEAD) in `taskWorktrees`.
3. **Task delivery**: persist committing intent and verified snapshot before committing; persist committed and pinned SHA before integration merge; persist merged before non-forced checkout cleanup, retain branch, then save DONE. Failures preserve artifacts. Continue/retry reconciles existing Git results before any model rerun.
4. **Merge failure**: create a resolver only for actual unmerged files with MERGE_HEAD matching this source. Locks, hooks, permissions and missing branches block. Resolver uses a private clean conflict baseline, inherits the model and limits writes to conflict files; unresolved markers block delivery. Original task completion requires persisted resolver identity and dependency changes. See [m1d](../reliability-m1d.md).
5. **Delivery** (after all tasks `DONE`, see §9): full gate in integration worktree → delivery summary → user chooses one of three (merge current branch / export patch / keep branch). **Default does not change user’s main checkout**.

## 7. Testing Phase

Trailing `testing` domain tasks usually depend on all implementation tasks:

- Role mapping (F13.e): `domain == "testing"` → `TESTER`, `"review"` → `REVIEWER`, else `EXECUTOR` (written to `ChatRequest.role` and `tasks.role`).
- Matching assigns low-cost models meeting `testing` domain bar.
- Brief includes contracts (test basis) and all upstream change lists, not implementation conversation; fixed header `TASK_BRIEF_FIXED_HEADER` unchanged (R10); test tasks append optional “### Defects found” in task report format section.
- Gate commands are project full test commands.

If test finds implementation defects (test correct but implementation violates contract), test model lists defects in report (each line `- <implementation task id>: <description>`); orchestration calls `generate_implementation_fix_task` for `<id>-fix` (depends on test task, `write_scope` from implementation task), not let test model edit implementation. Re-test after fix is user re-running test task.

## 8. Repair Fallback

Goal is first-time success; repair is safety net; its rate (1 − first-pass rate) is core matching quality metric.

Production path: ordinary gate failure → `decide_repair_action` (optional `is_provider_melted`: when true skip same window, reassign directly) → `repairSteps[taskId]` persisted; each step logs “Step N: same-window repair / reassign / block”.

```
Ordinary gate failure (scope/evidence faults block directly)
  │
  ├─ Step 1: repair in same execution window (VERIFYING → REPAIRING → RUNNING)
  │     resume = threadIds[taskId]; phase = REPAIR; append failure evidence
  │     no thread id → degrade to step 2
  │
  ├─ Step 2: still fail → `reassign_task` (exclude current model)
  │     new window (no resume), brief with failure evidence and prior change list
  │
  └─ Step 3: still fail → BLOCKED
        options: replan to split task / user terminal takeover / skip
```

### Terminal takeover (S1 / F10)

`BLOCKED` task can “Terminal takeover” on board:

1. IPC `terminal_resume_command(provider, threadId, cwd)` produces one paste line: `cd '<task cwd>' && <CLI> …` (each Adapter `terminal_resume`, see [11 §4.2](../11-command-authorization/README.md) and fix-todo A.15). No thread id / Aider etc. without resume-by-id shows “No resumable session” and hint.
2. User edits in external terminal, clicks “Takeover done, re-run gate” → `BLOCKED → VERIFYING` → shared `runGatesForTask` (task cwd, all acceptance commands) → pass `VERIFYING → DONE` and advance downstream; fail back to `BLOCKED`. No commands finish execution without automatic acceptance.
3. `RUNNING` tasks no longer offer “Mark complete” (illegal `RUNNING → DONE`).

Also: subagent sensitive command approval → `BLOCKED` (“Waiting for approval”), unrelated to repair ladder; after approve/deny `resume` same CLI session ([11 §4.3](../11-command-authorization/README.md)), then still gate / repair path above.

- Each step outcome written to `task_outcomes` (F14); matching updates success probability for that model in that domain.
- Per-task repair cost counts toward plan cost; when plan total repair cost exceeds 30% of estimate, budget red line ([09 §6](../09-guardrails/README.md)).

## 9. Plan Completion

After execution ends:

1. Run the deduplicated task commands in the actual integration worktree (or inplace directory), retaining every outcome. Missing commands, failure, cancellation or skipped tasks cannot authorize normal merge.
2. Summarize reports from the correct session/plan. Missing gate evidence never means success.
3. User merge requires all tasks DONE, ready, explicit passing acceptance and complete evidence. Recheck task definitions, states, commands and Git content; native code checks again and merges the accepted SHA. Changed content requires new acceptance; ordinary confirmation cannot override failure.
4. Persist merged and user acceptance time before cleanup. Patch export/branch retention preserve the acceptance verdict. See [m1f](../reliability-m1f.md).

> Delivery three-way UI (board) and orphan worktree cleanup (Settings → Execution): F11.f / F11.g.

## 10. Resume After Interrupt

- Pipeline state persisted to `.tokage/execution/<sessionId>.json` (`PipelineState` schema v2: full `taskItems`, contracts, upstream artifacts, per-task `threadId` / `cwd` / `allowedCommands`, etc.).
- After app restart: tasks in `RUNNING` / `VERIFYING` / `REPAIRING` had processes reaped; Rust `load_pipeline_state` sets `INTERRUPTED`, pipeline state also `INTERRUPTED`; **does not auto re-dispatch**.
- User clicks “Continue” on board → `continueTask`: with CLI `threadId` resume with prompt “Continue the unfinished task; if last turn finished, output the task report directly”; without, new window rerun with fresh brief. cwd must match task identity; missing worktree metadata blocks instead of falling back to inplace. Persisted Git intent is reconciled first; unknown intermediate states preserve artifacts.
- `PENDING` / `READY` keep assignment and deps; v1 old files that cannot backfill task defs from current plan → mark unrecoverable, discard only.
