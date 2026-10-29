# 05 · Context Lifecycle Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

Decision wiring checklist (checkable): [decision-wire-todo.md](decision-wire-todo.md)

## 1. Responsibilities

1. Assemble a concise task brief for each task as the first input to the execution window.
2. Decide whether a task reuses an existing window or opens a new one (window reuse).
3. During long tasks or user follow-up in a window, continue, compact, or open a new window in the most cost-effective way per model cache and context characteristics.
4. Ensure no Tokage operation breaks the CLI’s Prompt Cache.

## 2. Task-Level Windows

The plan-and-dispatch architecture naturally avoids “one window getting longer and longer”:

- Each task’s window contains only: CLI baseline (system prompt, tool definitions, project notes) + task brief + this task’s work process.
- Tasks pass results via task reports (≤ 300 Token), not conversation history.
- Contracts live as files; execution models read on demand; multiple tasks share the same file.

So context management shifts from “when to trim long history” to “keep briefs small and windows reusing hot cache.”

The main session has no context window. Its state is “current design draft + current contracts + task reports,” not conversation log; no model reads the main session’s full history. That state is rewritten, not appended ([02 §9](../02-planning-engine/README.md)), so main session length has no cap.

## 3. Task Brief

Composition (fixed order, fixed header first for cache hits):

| Part | Size |
|------|------|
| Fixed execution instructions (role, report format, honor contracts, stay within write scope) | Fixed, ~400 Token |
| Task card | 100–300 Token |
| Contract refs: file paths; short contracts (< 500 Token) inlined | By reference volume |
| Upstream task reports | Each ≤ 300 Token |
| Upstream change list | File path list |
| Failure evidence (repair reassignment only) | ≤ 1500 Token |

Target total brief ≤ 3K Token. Template: [task-brief.md](task-brief.md).

## 4. Window Reuse

When a model just finished task A and is assigned task B, it may continue B in A’s window instead of opening a new one:

```
Reuse cost  = Σ C_turn(B + H_A + D_B + i·g, warm_i)        H_A = A window history
New cost    = Σ C_turn(B + D_B + i·g, warm_i)  +  re-read cost

Reuse conditions (all must hold):
  - Same model, same role and permissions (permission change requires new window, see §8)
  - A and B same domain or adjacent write scope (A’s history useful for B)
  - Gap from A end to B start < cache TTL (cache hot on reuse)
  - Reuse cost < new cost × (1 − min_saving)
  - B + H_A below effective context hard limit
```

“Re-read cost” is the portion a new window re-reads to re-understand code A already saw, `φ × H_A_relevant × P_in × w` (`φ` default 0.3). Zero when domain and scope unrelated; reuse will not be selected.

Matching engine considers reuse during global allocation ([04 §7](../04-model-matching-engine/README.md)); orchestration engine confirms at task start from real state.

## 5. In-Window Context Planning

Applies to: multi-turn execution of large tasks, user follow-up in an execution window. Before each turn, compare cost among candidate actions:

| Action | Execution |
|------|---------|
| `CONTINUE` | Send with `resume_id` |
| `COMPACT` | Invoke CLI native compact (with “keep current task goal, contract paths, unresolved errors”), then send |
| `FRESH` | New window, first input = reassembled task brief + current progress report + this message |

Cost (`C_turn` defined in [04 §6.2](../04-model-matching-engine/README.md), `h` = remaining expected turns):

```
CONTINUE = Σ_{i<h} C_turn(B + H + i·g, warm_i)            warm_0 = whether current cache still hot
COMPACT  = K_compact + Σ_{i<h} C_turn(B + S + i·g, warm_i) + 0.1 × re-read cost
           S = H × compact_output_ratio, K_compact = read full history + output summary
FRESH    = K_report  + Σ_{i<h} C_turn(B + D' + i·g, warm_i) + 0.3 × re-read cost
```

Decision rules:

1. Hard constraint: when `B + H + h·g` exceeds `min(effective_context, context_window × cli_auto_compact_ratio) × 0.9`, exclude `CONTINUE`, act before model quality drops or CLI auto-compact triggers.
2. Otherwise sort by cost; only leave `CONTINUE` when savings ≥ 15%.
3. At least 2 turns between two non-`CONTINUE` actions in the same window.

Cache temperature:

```
warm_now = (now − last_turn_end) < ttl × 0.9
```

Planning runs before user message is sent, so interval is known and hot/cold is deterministic. `disk_persistent` cache almost always hot; `implicit` cache discounted by `hit_reliability`.

### 5.1 Worked Example

`r = 0.1`, `w = 1.25`, TTL 5 minutes, `B = 25K`, `g = 8K`, `k = 6`, `compact_output_ratio = 0.06`, units: K Token × `P_in` (output same across options, omitted).

| Scenario | CONTINUE | COMPACT | Conclusion |
|------|---------|---------|------|
| `H = 90K`, `h = 4`, user away 12 min (cold) | ≈ 613 | ≈ 378 | 38% savings, compact |
| `H = 15K`, `h = 2`, user replies within 1 min (hot) | ≈ 89 | ≈ 80 | 11% savings, below threshold, continue |

Same history length: models with persistent cache and very low read price favor continue; on no-cache models, `k` full-price re-reads per turn make compact worthwhile sooner.

## 6. User Follow-Up Triage

When user adds a message in the main session, local triage decides destination (`triage_main_session_message_cmd`, Composer calls when pipeline not finished):

| Judgment | Destination |
|------|------|
| Detail change to a specific task (mentions files in its write scope or task name) | That task’s execution window: queue follow-up, inject when task continues |
| New requirement or cross-task change | Planning engine incremental replan ([02 §9](../02-planning-engine/README.md)); running pipeline must pause first |
| Follow-up on last direct dispatch answer (anaphora, or short gap and same topic) | Prefer continue last direct dispatch model |
| Simple question | Direct dispatch ([04 §8](../04-model-matching-engine/README.md)), new window |

User can explicitly pick target task in the input box.

**Wiring (2026-10-08):** `dispatchSubAgentTask` calls `evaluateWindowReuse` (`findReusableWindow`) before opening new window: for same model / same role, adjacent domain or write scope, cache still hot, DONE tasks continue their CLI thread; under worktree isolation reuse only when peer working directory is session baseline. R9/R10 discipline enforced via `validate_*_cmd` after brief assembly and on resume path.

## 7. Execution Modes

| `context.mode` | Behavior |
|----------------|------|
| `auto` | Opt-in: queue a supported ordinary-main-session COMPACT recommendation automatically, without a model call. The next ordinary user send revalidates and consumes it; undo before send suppresses automatic requeue for that source turn. Unsupported native/FRESH-only recommendations remain unavailable. |
| `suggest` | For supported ordinary main sessions, a COMPACT recommendation can be explicitly queued and cancelled before the next turn. The next turn sends retained material as an instruction or starts a new window with a brief. Native slash commands are unavailable until M2. |
| `off` (default) | No context planning or intervention. |

In-window planning passes `contextMinSaving` and `cacheAware` (when off, estimate as no cache). Queueing records SUGGESTED; undo records UNDONE without a cooldown anchor. Preparing the next ordinary carrier revalidates and binds that same row as APPLIED. Cancelling while preparation is in flight marks the prepared carrier UNDONE. Approval/repair follow-ups cannot consume a user action. Completing the carrier proves neither native compaction nor actual savings. For a new window with a reported USD cost, the CONTEXT ledger compares the planner's estimated continuing first-turn cost with the reported new-window first-turn cost. This is a counterfactual estimate, not measured savings; instruction-only and unknown-cost results remain unsettled. Historical CONTEXT records using the old basis are retained but excluded from the dashboard. Shared Runtime main sessions are read-only here; task windows and native actions remain later milestones. See [M1.e](m1e-execution.md) and [M1.f](m1f-settlement.md).

## 8. Cache-Preservation Discipline

- Do not switch model, permissions, or launch args within the same window; always new window when needed.
- Fixed parts of task brief and execution instructions byte-identical; no timestamps or volatile inserts.
- User messages sent verbatim; no prepended status.
- No extra requests solely to keep cache warm.

**Production path:** `validate_task_brief_header_cmd` (before new-window dispatch), `validate_window_continuity_cmd` (same-window repair / continue / approval resume), `validate_user_message_integrity_cmd` (when injecting follow-up on continue). Violations → task BLOCKED, CLI not started.

## 9. Configuration

| Key | Default | Description |
|----|------|------|
| `context.mode` | `off` | Explicit opt-in. Settings v7 maps historical manual-only auto to suggest; off/suggest stay unchanged. Newly selected auto is retained. |
| `context.min_saving` | `0.15` | Minimum savings ratio to leave CONTINUE / choose reuse |
| `context.brief_max_tokens` | `3000` | Task brief cap |
| `context.report_max_tokens` | `300` | Task report cap |
| `context.rework_phi` | `0.3` | Re-read cost coefficient |
