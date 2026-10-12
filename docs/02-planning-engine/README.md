# 02 · Planning Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

1. Triage: decide whether the request should dispatch a single task directly or enter planning first.
2. Generate a project digest (local, zero token) as the main input to the planning model.
3. Select a planning model, invoke read-only, produce a concise plan: design + contracts + task graph.
4. Validate plan structure, persist as project spec files, hand off for user review.
5. Support incremental replanning: when requirements change, tasks fail repeatedly, or the user edits, update only affected parts.

## 2. Triage

Done locally, zero token:

| Signal | Lean toward |
|------|------|
| Multiple domains (UI and API, database, deploy mentioned together) | Plan |
| New feature / module-level wording (“implement”, “build”, “refactor entire …”) | Plan |
| Expected file count > 3 (paths, module names matched against project digest) | Plan |
| Single issue (“this error”, “explain”, “change this function”) | Direct |
| Short message pointing at a specific location | Direct |

Result is `DIRECT` or `PLAN`; when confidence is low, default to a light form of `PLAN` (planner may return a single task). User can choose “Auto / Always plan / Direct execute” in the input.

`DIRECT` path: matching engine picks one model for the task; task card generated locally (original request + related files); skip planning model.

## 3. Project Digest (RepoDigest)

The planning model’s largest token cost is usually “exploring the codebase”. Tokage pre-generates a digest locally so the planner barely needs to open files:

| Content | How generated |
|------|---------|
| Directory tree (respect `.gitignore`, fold large dirs, annotate file counts) | Walk |
| Languages & frameworks, package manager, main deps and versions | Parse `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, etc. |
| Build / typecheck / test / lint commands | From manifest scripts, Makefile, CI config |
| Public symbols: exported types, function signatures, routes, DB models | Line-prefix symbol extraction (first 300 lines per file), signatures only |
| Existing specs: project docs (`CLAUDE.md`, `AGENTS.md`, `README`) summary | Headings and bullet points |
| Recent change hotspots | `git log --stat` last 50 commits |

- Digest cached per project; incremental update on `notify` changes.
- Default size cap 8K tokens, trimmed by “relevance to request”: modules mentioned in the request keep full signatures; rest keep directory levels only.
- Digest content is stable and placed at the front of planning input so repeated planning on the same project hits cache.
- **Wiring (F16)**: Plan mode first turn uses frontend `getRepoDigest` + `renderPlanningPrompt({ userRequest, repoDigest, domains })`; `framed_prompt` keeps only the read-only constraint line. Single source for plan format spec: `tokage-planning::PLAN_FORMAT_SPEC`; format repair via `render_plan_repair_prompt`. `validatePlan` passes `allowedCommands = digest.commands` union.

## 4. Choosing the Planning Model

Matching engine selects by role `planner` ([04 §5](../04-model-matching-engine/README.md)):

- Choose only among models meeting minimum planning capability; definition pack usually lists each vendor’s strongest reasoning models (e.g. Opus, Fable class).
- Planner output is short but quality determines the whole pipeline; minimum planner capability is higher than any execution task; price is not the primary factor.
- User can pin a planning model in settings.

## 5. Planning Invocation

- Role `PLANNER`: read-only; file read and search allowed; no write or command execution.
- Input: fixed planning instructions (cache-friendly) + project digest + user request + available domain list (no specific models—planner does not pick models).
- Read budget: allow reading a few key files on demand; instructions require preferring the digest.
- Output cap: default 6K tokens; write “constraints” only, not implementation code; contracts as signatures, types, table schemas.
- Timeout class `PLANNING` (see [09 §2](../09-guardrails/README.md)).

Full output format and planning prompt template: [plan-format.md](plan-format.md).

## 6. Plan Structure

```
Plan
 ├─ Design (spec): goals, scope, key decisions, non-goals (concise Markdown)
 ├─ Contracts: API definitions, data structures, component props, directory & naming conventions
 └─ Task graph (tasks):
      T1 [backend-api]  depends: —      write: src/server/**   acceptance: cargo test -p server
      T2 [frontend-ui]  depends: —      write: web/src/**      acceptance: pnpm typecheck
      T3 [integration]  depends: T1,T2  write: web/src/api/**  acceptance: pnpm test api
      T4 [testing]      depends: T3     write: tests/**        acceptance: pnpm test && cargo test
```

Each task includes: domain, title, goal, dependencies, referenced contracts, write scope, deliverables, acceptance commands and criteria, size (S/M/L), difficulty (1–3).

Persistence:

- Current implementation: each session’s plan written to `.tokage/plans/<session-id>.md` with history; directory layout and boundaries see [08 §1.1](../08-storage/README.md).
- After structured plan (P5): `.tokage/plans/<plan_id>/` with `spec.md`, `contracts/*`, `tasks.yaml`.
- Execution models read contracts from files, not full contract text in every task brief; contracts are single source of truth for all parallel tasks.
- User can disable on-disk persistence in settings (contracts inlined in briefs, higher token cost).

## 7. Plan Validation

Planner output is validated locally before review:

| Check | On failure |
|------|---------|
| Parseable structure (YAML / Markdown sections complete) | Local tolerant repair (default missing fields); still fail → ask planner to resend task graph only |
| Task graph acyclic, dependency refs exist | Same |
| Domains in definition pack domain table | Map to nearest domain or `general` |
| Parallel tasks (no dependency) write scopes do not overlap | Auto-add dependency to serialize, or prompt user |
| Each task has at least one acceptance criterion | Fill from digest default build/test commands |
| Acceptance commands match allowlist | Non-matching commands highlighted; user must confirm ([09 §5](../09-guardrails/README.md)) |
| Task count ≤ 20 | Prompt to split into phases |

## 8. User Review

Plan shown as: design, contracts, task graph (with matching engine model assignment and estimated cost). User can:

- Confirm and execute.
- Edit tasks (merge, split, change deps, write scope), edit contracts.
- Assign models per task.
- Request changes, triggering incremental replanning.

**On plan confirm** (F1.f / S6): recompute model assignment once (manual picks as `overrides`), write each task’s `assigned_model` as `<ProviderId>:<cliModelArg>` (e.g. `CLAUDE_CODE:sonnet`), persist plan, and pass full `TaskAssignment` snapshot to pipeline state for learning and settlement. Unparseable assignments make the task `BLOCKED` at dispatch, no silent fallback to “first available model”.

Settings `requireReview` / `planningAutoApproveBelowUsd`: when validation passes with no Error, if forced review is off or total cost below threshold, review UI auto-confirms (undo before it takes effect). When `plannerModel` ≠ `"auto"`, Plan first turn uses that model.

## 9. Incremental Replanning

Triggers: user adds out-of-scope requirements, repair ladder escalates to planning ([03 §8](../03-orchestration-engine/README.md)), user asks (plan page “Request changes”, or main session triage hits replan keywords).

- Input: current design and contracts (usually still cache-hit) + reports of completed tasks affected by this change + change description. Unaffected tasks only id, title, status. Assembled by `assembleReplanPrompt`.
- Total input bounded by `planning.replan_input_max_tokens`; when over, trim reports to “what was completed” first, then drop reports by relevance to the change.
- Output: full proposal parsed from Plan turn, then `computePlanDiff`; UI shows `PlanDiffView`; on confirm `applyPlanDiff` (version +1) and `generateAdaptationTasks`.
- Completed unaffected tasks unchanged; completed tasks affected by contract changes get “adaptation tasks”.
- After applying diff, design and contracts are rewritten wholesale to latest version, not appended revision notes. Each replan creates a new plan version; old versions archived only, not fed into later replan input. Replanning only when pipeline not started or paused.

## 10. Configuration

| Key | Default | Description |
|----|------|------|
| `planning.mode` | `auto` | `auto` / `always` / `direct` |
| `planning.planner_model` | auto | Pin planning model |
| `planning.digest_max_tokens` | `8000` | Project digest cap |
| `planning.output_max_tokens` | `6000` | Planning output cap |
| `planning.replan_input_max_tokens` | `16000` | Incremental replan input cap (excludes project digest) |
| `planning.max_tasks` | `20` | |
| `planning.write_to_project` | `true` | Persist plan and contracts under `.tokage/` |
| `planning.auto_approve_below_usd` | off | |
