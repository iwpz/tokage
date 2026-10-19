# 09 · Guardrails & Safety Red Lines

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

This chapter’s constraints are enforced in the Rust core. Settings may tune parameters but cannot disable the red lines themselves.

Reliability acceptance (2026-10-19): [m2g](../reliability-m2g.md) distinguishes layered faults from full desktop SIGKILL. Uncertain CLI outcomes block after recovery and preserve their checkout; recovery does not automatically redispatch or deliver, and missing billing remains unknown. See the acceptance matrix for actual scope and untested cases.

## 1. Output silence and filtering

### 1.1 Goal

Only informative text may enter logs, the frontend, task reports, and token estimates. Progress bars, spinners, and `\r` overwrite lines are never persisted.

### 1.2 Reduce noise at the source

- Prefer the CLI’s structured output modes; structured events do not include progress bars.
- Headless child processes set `NO_COLOR=1`, `TERM=dumb`; PTY mode keeps the real `TERM`.
- `CI=1` can disable interactive progress bars in many tools but may change CLI behavior; each Adapter declares whether it sets this.

### 1.3 OutputFilter pipeline

```
bytes
 → UTF-8 decode (invalid bytes replaced with U+FFFD)
 → CR folding: split on \n; lines containing \r keep only the segment after the last \r
 → ANSI stripping: vte parse, drop CSI / OSC sequences
 → Noise rules: drop spinner lines, progress patterns (e.g. ^\s*\[?[=#>\-\s]*\]?\s*\d{1,3}%); fold consecutive duplicate lines as “(×N)”
 → Length cap: single chunk > 64KB keeps first 16KB + last 16KB
 → FilteredChunk
```

Noise rules ship with definition packs; Adapters may append provider-specific rules. Acceptance command output uses the same pipeline.

## 2. Timeouts

### 2.1 Two-layer timeouts

An agent-style CLI turn may include dozens of tool calls; long total duration does not mean stuck—stuck means no output for a long time. Therefore both idle and hard timeouts apply:

| Tier | Idle timeout | Hard timeout | Typical use |
|------|-------------|--------------|-------------|
| `QUICK` | 15 s | 30 s | Usage queries, compaction |
| `STANDARD` | 90 s | 180 s | S / M development and test tasks |
| `LONG` | 180 s | 420 s | L tasks, full-repo audit |
| `PLANNING` | 300 s | 900 s | Planning |
| `GATE` | 120 s | 300 s | Single acceptance command |
| `AGENT` | Setting `agentIdleTimeoutMin` (default 30 min) | Setting `agentHardTimeoutMin` (default 120 min) | Pipeline sub-agent turns; absolute values, no timeout multiplier |

- Except `AGENT`, execution task tier follows size; user may scale globally (1× / 2× / 4×). `AGENT` idle / hard come from settings and must satisfy `hard ≥ idle`.
- Hard timeout applies per turn; multiple turns on one task each have their own timer.
- While waiting for approval, **only idle timing pauses**; hard timeout starts at turn begin and never pauses.

### 2.2 Implementation

```rust
async fn supervise(mut child: Child, group: ProcGroup, limits: Limits, mut lines: LineStream,
                   approval_pending: bool) -> RunOutcome {
    let hard = tokio::time::sleep(limits.hard);
    tokio::pin!(hard);
    loop {
        if approval_pending {
            // Idle paused; hard still armed.
            tokio::select! {
                _ = &mut hard => { group.kill().await; return RunOutcome::Timeout(TimeoutKind::Hard); }
                line = lines.next() => match line {
                    Some(line) => forward(line),
                    None => break,
                },
            }
        } else {
            tokio::select! {
                _ = &mut hard => { group.kill().await; return RunOutcome::Timeout(TimeoutKind::Hard); }
                r = tokio::time::timeout(limits.idle, lines.next()) => match r {
                    Err(_)         => { group.kill().await; return RunOutcome::Timeout(TimeoutKind::Idle); }
                    Ok(Some(line)) => forward(line),
                    Ok(None)       => break,
                },
            }
        }
    }
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => RunOutcome::Exited(status),
        _              => { group.kill().await; RunOutcome::Timeout(TimeoutKind::Exit) }
    }
}
```

### 2.3 Process group cleanup

npm-installed CLIs are usually `node` wrappers that spawn shells, compilers, and other grandchildren—the whole group must be reaped:

- Unix: `setsid()` in `pre_exec` on spawn; on cleanup `killpg(pgid, SIGTERM)`, then `SIGKILL` after 3 s if still alive.
- Windows: Job Object + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; on cleanup `TerminateJobObject`.
- Tokage abnormal exit: on Unix record running pgids in `run/children.json`, clean leftovers on next startup; Windows relies on Job handle close for automatic cleanup.
- **Cancel (user “Stop” / `cancel_chat_turn`)**: cooperative cleanup—`TurnRegistry` notifies `run_cli` via `watch`, which `group.kill()` + `child.kill()` the process group; `cancel` waits for the turn task to finish (confirm process group exited) before returning; after 8 s, `abort` as fallback. Usage listening is an independent bus subscription, not the cancellable task. Frontend marks `cancelled` only after `cancelChatTurn` succeeds; `sendQueuedNow` dispatches new messages only after cancel completes.

### 2.4 After timeout

1. Reap process group, set attempt `TIMEOUT`, write `SYSTEM` log; task `BLOCKED` (reason states idle / hard timeout).
2. `run_turn` publishes `TurnFailed { kind: Timeout }`; app-layer subscribers increment `provider_health.consecutive_fails`, trigger `HEALTH` melt at 3 ([06 §6](../06-usage-melting-engine/README.md)). User-cancelled turns do not emit `TurnFailed` / `TurnSucceeded` and do not count as failure.
3. Task goes to orchestration repair fallback ([03 §8](../03-orchestration-engine/README.md)): timeout counts as failure; evidence is timeout kind and last output.
4. Terminated CLI may have partially changed files; change list goes into failure evidence; later executors continue from current state.

## 3. Concurrency and resources

| Constraint | Default | Notes |
|------------|---------|-------|
| Global max concurrent child processes | Setting `maxParallel` (default 3; scheduler internal floor 1) | Maps to `pick_dispatchable_tasks.maxGlobalConcurrent`; does not starve local builds |
| Max concurrent per Provider | Pack `providers.<id>.concurrency_hint`, default 2 | `pick_dispatchable_tasks` reads effective pack; avoids rate limits |
| Parallel task write scopes | Must not overlap | See §4; occupancy below |
| Child process priority | `nice +5` | Below user foreground work |
| Acceptance command concurrency | 1 | Builds and tests usually monopolize resources |

Occupancy (orchestration scheduler D1 / [03 §3](../03-orchestration-engine/README.md)): global and write scope count `RUNNING` + `VERIFYING` + `REPAIRING`; per Provider counts `RUNNING` + `REPAIRING` (verification does not consume Provider slot).

## 4. Write scope

- When creating an execution window, if the CLI supports path-level write limits (cap `WRITE_SCOPE`), apply task `write_scope`.
- Save a Git disk/index baseline before launch; verify before/after gates, after report storage and before commit. Tracked and non-ignored untracked bytes, modes, deletions and index changes are authoritative; CLI events cannot shrink the set.
- Shared inplace checkout runs serially; ambiguous ownership across frontend sessions blocks. Other task scopes never exclude edits. Directory, HEAD, baseline, scope IPC or Git read faults block commit/DONE/downstream.
- Worktree commit validates accepted content, staged blob/mode and final commit tree. Filters, hooks or external edits that change content preserve artifacts and block delivery.
- `worktree` cleanup (F11.g): remove task worktree after successful merge (keep branch until delivery); `git worktree prune` on startup; orphan dirs under `<app data>/worktrees/` listed in Settings → Execution, force-clean after user confirm (still confirm when `DIRTY`).
- Planning: if parallel tasks (no transitive dependency) have overlapping `write_scope`, `validate_plan` by default auto-adds dependencies to serialize (`parallel_write_conflict_auto_resolved`).
- Restore requires user confirmation of exact paths, turnId-bound pre-turn byte/index backups and evidence sealed after real process exit. Save recovery copies before overwrite/deletion. Later user edits, missing evidence or partial failure report errors and preserve artifacts; checkout/clean cannot replace user work. See [m1e](../reliability-m1e.md).
- Planning and review windows are read-only; any detected change terminates immediately with alert.

Snapshots are not a whole-filesystem sandbox: ignored/outside-repository files and network/database effects are excluded; scans cap at 50,000 files/512 MiB. Backups exclude ACLs, xattrs, ownership and directory metadata. Cross-process ownership and external write races remain m2e work; arbitrary crashes are not automatically recoverable. See [m1c](../reliability-m1c.md) and [m1g acceptance](../reliability-m1g.md).

## 5. Command safety

- Child commands are built only by Adapters; user input and task briefs pass as separate args or stdin, never via `sh -c`.
- Acceptance commands come from planner output; before run they must satisfy (R5, allowlist from effective pack `gate_allowlist`, no code constants):
  - Match `gate_allowlist`, or be in task `acceptance.confirmedCommands` (review UI confirms each line).
  - No pipes, redirects, command substitution, `;` / `&&` chaining (meta chars rejected even if confirmed); execute as argv array.
  - Working directory limited to project root or task worktree; global acceptance concurrency 1.
- Tokage does not enable CLI “skip all permission prompts” flags for the user; `full_auto` is implemented as “broad allow list + red-line deny + OS sandbox” ([11 §2](../11-command-authorization/README.md)).
- Commands initiated inside the CLI follow [11](../11-command-authorization/README.md) authorization mode and command lists; this section only constrains commands Tokage runs itself.

## 6. Budget red lines

| Red line | Default | Action |
|----------|---------|--------|
| Plan actual cost over estimate | 50% | Pause queue only if `pauseOnOverrun` true; otherwise notify only |
| Repair cost vs plan estimate | 30% | Pause only if `pauseOnOverrun` true; otherwise notify only |
| `planBudgetUsd` hard cap (>0) | User setting | **Always** pause queue (ignores `pauseOnOverrun`) |
| Pay-as-you-go Provider daily spend | User setting | Melt (`BUDGET`) |

Wiring (F13.f): after task ends `plan_cost_summary` (sum `COALESCE(cost_usd, equiv_cost_usd)` by `phase`) + `check_plan_budget_overrun`; on hard or soft red line when pause applies, call `pauseExecution`.

## 7. Red line checklist

| # | Red line | Enforcement site |
|---|----------|------------------|
| R1 | Raw text with `\r` overwrite, ANSI, spinner, progress bars not persisted | OutputFilter |
| R2 | Every child runs under supervise with idle and hard timeout | NativeEngine |
| R3 | Timeout / cancel reaps entire process group | NativeEngine |
| R4 | No shell string concatenation to build commands | ProviderAdapter / Gate |
| R5 | Acceptance commands must be on allowlist or user-confirmed | OrchestrationEngine |
| R6 | Parallel task write scopes do not overlap; scope check after every task | OrchestrationEngine |
| R7 | Planning and review windows read-only | NativeEngine |
| R8 | Do not enable CLI skip-permission flags for user; full auto only via lists and sandbox | ProviderAdapter |
| R9 | Same window: no model switch, no permission or launch arg changes | ContextEngine |
| R10 | Context actions only at turn boundaries; no extra requests for keep-warm | ContextEngine |
| R11 | Tokage reads only non-secret CLI config fields (Base URL, auth method, plan); Key / Token values discarded on read, not persisted, not logged, not sent to frontend; Tokage does not auth / query quotas against model vendors (except pack download and update check); UI prompts user to verify install source ([11 §7](../11-command-authorization/README.md)) | NativeEngine / frontend |
| R12 | Pause queue when budget red lines exceeded | OrchestrationEngine |
| R13 | Model-initiated commands follow auth mode; `forbidden` denied in all modes | AuthEngine ([11](../11-command-authorization/README.md)) |
| R14 | EXECUTOR-role CLI process trees run in OS sandbox (Windows v1 exception; full auto not offered) | NativeEngine |
| R15 | Model cannot read credential paths or other CLIs’ config dirs | Sandbox / shell wrap / tool rules |
| R16 | Child env trimmed per vendor; model shell must not expose credential vars | NativeEngine |
| R17 | Secret values redacted in logs and UI; alert when Key written to project files | OutputFilter |
| R18 | Local proxy CONNECT forward only, no decrypt, no content logging; upstream proxy creds not in child env | ProxyRelay ([12](../12-network-proxy/README.md)) |

Each red line has unit or integration tests using `FakeAdapter` to inject hang, spam, grandchild spawn, out-of-scope write, read-only window file write, credential path read, env print, `forbidden` command, etc.

> Current state (2026-10-08): R1–R18 largely implemented with crate tests. Per [fix-todo](../fix-todo.md) verification—**R5 / R6 / R16 production-path tests are valid only after F5.k** (via `processSubAgentCompletion` / gate dispatch entry, not function-only); **R9 / R10** enforced on dispatch / resume via `validate_*_cmd` in `taskRunner` (fixed brief header, window continuity, follow-up verbatim). Other red lines validated per crate tests and fix-todo items; no claim of “100% end-to-end green” without distinguishing library vs production-path tests.
