# 03 · Orchestration Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

1. Turn the plan’s task graph into an executable queue; schedule by dependencies and resource constraints.
2. Pass artifacts between tasks: upstream task reports and changes become downstream input.
3. Run acceptance gates locally after each task completes.
4. On gate failure, run repair fallback and feed results to the matching engine for learning.
5. Support pause, resume, cancel, retry, and resume-after-restart from checkpoint.

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
- Before gate, R6: `changedFiles` must fall within `write_scope` (`inplace` excludes files covered by overlapping scopes of other concurrent tasks); out of scope → skip gate, task `BLOCKED`, one-click revert ([09 §4](../09-guardrails/README.md)).
- `acceptance.criteria` self-checked by executor in task report; Tokage does not judge.

All pass → `DONE`, counted as “first pass” if completed within first execution window.

## 6. Workspace Isolation

Code values: `inplace` | `worktree` (setting `isolation`). Older docs called `inplace` `shared`.

| Mode | Description | Default |
|------|------|------|
| `inplace` | All tasks at project root; parallel safety via non-overlapping write scopes | Optional |
| `worktree` | Per-task worktree; Tokage commits and merges into session integration branch | Yes (F11.i; design default) |

### worktree flow (F11)

Naming: `sid8` = first 8 alphanumeric chars of session id; integration branch `tokage/<sid8>-integration`; task branch `tokage/<sid8>-<taskId>-<n>` (`n` = attempt number). Directories under `<app data>/worktrees/<repo>-<hash>/<name>`.

1. **Session start**: before first dispatch `ensureSessionIntegration` creates integration branch and worktree from `HEAD`, writes `PipelineState.integration`.
2. **Task dispatch**: `create_session_worktree(..., baseRef = integration.branch)`; record `baseCommit` (integration HEAD) in `taskWorktrees`.
3. **After gate pass**: `commit_task_worktree` → session serial lock → `merge_task_worktree_cmd` (only paths under app `worktrees/`) → on success clean task worktree dir (**keep branch**) → `DONE`.
4. **Merge conflict**: returns `Conflict`; original task still `DONE`; inject `{id}-integration` (`depends_on = [original]`, `write_scope` = conflict files); downstream adds dep on integration task; `prepare_integration_worktree` from integration branch and `merge --no-commit --no-ff <task branch>` (conflict markers for CLI); after pass commit and merge to integration.
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

Production path (F13.c): gate fail or R6 out of scope → `decide_repair_action` (optional `is_provider_melted`: when true skip same window, reassign directly) → `repairSteps[taskId]` persisted; each step logs “Step N: same-window repair / reassign / block”.

```
Gate fail / R6 out of scope
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
2. User edits in external terminal, clicks “Takeover done, re-run gate” → `BLOCKED → VERIFYING` → shared `runGatesForTask` (task cwd, all acceptance commands) → pass `VERIFYING → DONE` and advance downstream; fail back to `BLOCKED`. No acceptance commands treated as pass.
3. `RUNNING` tasks no longer offer “Mark complete” (illegal `RUNNING → DONE`).

Also: subagent sensitive command approval → `BLOCKED` (“Waiting for approval”), unrelated to repair ladder; after approve/deny `resume` same CLI session ([11 §4.3](../11-command-authorization/README.md)), then still gate / repair path above.

- Each step outcome written to `task_outcomes` (F14); matching updates success probability for that model in that domain.
- Per-task repair cost counts toward plan cost; when plan total repair cost exceeds 30% of estimate, budget red line ([09 §6](../09-guardrails/README.md)).

## 9. Plan Completion

When all tasks `DONE` (or user skipped):

1. Run one full gate (project full build and test). In `worktree` mode inside **integration worktree** (prefer RepoDigest build/test; before F16 union of task acceptance commands deduped).
2. Generate delivery summary: per-task model, changes, acceptance, cost; plan total vs estimate; savings ([06 §5](../06-usage-melting-engine/README.md)) (IPC `generate_delivery_summary_cmd`).
3. `worktree` delivery three-way (default does not touch user main checkout):
   - **Merge to current branch**: only when main checkout clean and double-confirmed; `git merge --no-ff tokage/<sid8>-integration`; on conflict `merge --abort` and fall back to export patch.
   - **Export patch**: `git diff <base>..<integration branch>` to file.
   - **Keep branch**: clean worktree dirs, leave branches for user.
4. Optional: assign `review` domain task; reviewer read-only checks change summary against contracts (default off, suggested by plan size).

> Delivery three-way UI (board) and orphan worktree cleanup (Settings → Execution): F11.f / F11.g.

## 10. Resume After Interrupt

- Pipeline state persisted to `.tokage/execution/<sessionId>.json` (`PipelineState` schema v2: full `taskItems`, contracts, upstream artifacts, per-task `threadId` / `cwd` / `allowedCommands`, etc.).
- After app restart: tasks in `RUNNING` / `VERIFYING` / `REPAIRING` had processes reaped; Rust `load_pipeline_state` sets `INTERRUPTED`, pipeline state also `INTERRUPTED`; **does not auto re-dispatch**.
- User clicks “Continue” on board → `continueTask`: with CLI `threadId` resume with prompt “Continue the unfinished task; if last turn finished, output the task report directly”; without, new window rerun with fresh brief. cwd is task’s own work dir (fallback session `baseCwd`).
- `PENDING` / `READY` keep assignment and deps; v1 old files that cannot backfill task defs from current plan → mark unrecoverable, discard only.
