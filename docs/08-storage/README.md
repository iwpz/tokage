# 08 · Local Storage

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

> Reliability update (2026-10-16): execution artifacts use V12 identity constraints. Pipeline JSON has a separate serialized, revisioned atomic-save protocol; the generic store fallback below does not apply.

## 1. Basic configuration

- Location: Tauri `app_data_dir()/tokage.db`.
- On each connection:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

- Concurrency: a dedicated writer thread receives `mpsc` write commands, batching into one transaction every 100 ms or every 200 commands; reads use an `r2d2` pool (4 connections).
- Migrations: `refinery`, scripts at `src-tauri/crates/tokage-storage/migrations/V{n}__{name}.sql`, applied automatically on startup. If the database has pending migrations, first back up with `VACUUM INTO` to `app_data_dir()/backups/tokage-v{current-version}-{timestamp}.db`, keeping the latest 5 copies.
- Times are stored uniformly as UTC ISO 8601 text.
- Copies of files outside the project: files written during a turn outside the working directory are copied to `app_data_dir()/files/<project-folder-name>/<local-date>/<filename>`; the preview panel reads the copy; files inside the project are still read in place.

### 1.1 Project context directory `.tokage/`

Tokage maintains `.tokage/` at the project root (worktree root when the session uses a worktree). Each CLI reads the same context via `AGENTS.md` (implementation: `tokage-native/src/context.rs`).

```text
.tokage/
  context.md                          Constraints and task state, shared by all sessions
  plans/<session-id>.md               Current plan for the session (overwritten after each Plan turn)
  plans/history/<session-id>/<n>.md   Older versions, n increments from 1
  execution/<session-id>.json         Pipeline execution state (PipelineState v2, see below)
```

`execution/<session-id>.json` (F7): `schemaVersion: 2`, includes full `taskItems` / `contracts` / `upstreamArtifacts` / `assignments`, plus per-task `threadId`, `cwd`, `allowedCommands`, `approvals`, etc. Legacy v1 files without `schemaVersion` are treated as 1; on load, task definitions are backfilled from the same session’s plan; if backfill fails, the frontend marks the state unrecoverable. On cold start, `RUNNING` / `VERIFYING` / `REPAIRING` → `INTERRUPTED` (written back by Rust).

- Plan body text comes from the Plan turn’s reply text, not from the CLI’s built-in plan mode on disk; Claude Code Plan turns run in `default` permission mode, allowing read-only tools only, with edit tools and `ExitPlanMode` disabled.
- Filenames use session IDs; the title is on the first line of the file; later turns for the same session (any CLI) receive the plan path in the prompt.
- Write boundary: Tokage writes all of `.tokage/`; under the project root it only appends the block wrapped by `<!-- tokage:begin -->` / `<!-- tokage:end -->` in `AGENTS.md` (may be updated repeatedly), and one line `@AGENTS.md` in `CLAUDE.md`; `.gitignore` gets one line `.tokage/` only after user consent. CLIs read `.tokage/` only.
- On first creation of `.tokage/`, ask whether to add to `.gitignore`; choice is stored per project in `settings.tokageGitignore`.

## 2. Entity relationships

```
projects
 └─ main_sessions
     ├─ plans (version)
     │    ├─ plan_contracts
     │    └─ tasks ── task_edges (dependencies)
     │         ├─ task_assignments (assignment history)
     │         ├─ attempts (execution windows) ── chat_logs / usage_events / context_actions
     │         ├─ task_reports
     │         └─ task_outcomes (learning samples)
     └─ attempts (planning window, direct-dispatch window; task_id may be null)

Handoff text (briefs, progress reports) lives in briefs, linked to attempt

savings_events · usage_snapshots · provider_health                 usage and circuit breaking
provider_profiles · model_knowledge_base · model_offerings · knowledge_blobs   definition packs
model_overrides · custom_packs · learned_stats · triage_feedback · settings   local overrides and learning
command_rules · approvals                                           command authorization
```

## 3. Table overview

| Table | Purpose |
|-------|---------|
| `projects` | Project paths and metadata |
| `repo_digests` | Project digest cache |
| `main_sessions` | Main sessions |
| `plans` | Plan versions: spec, estimated cost, status |
| `plan_contracts` | Contracts |
| `tasks` / `task_edges` | Tasks and dependencies |
| `task_assignments` | Per assignment: model, success probability, estimated cost, rationale |
| `attempts` | Execution windows (one CLI session) |
| `briefs` | Task briefs and progress reports |
| `chat_logs` + `chat_logs_fts` | Filtered logs and full-text search |
| `task_reports` | Task reports |
| `gate_runs` | Acceptance command execution records |
| `task_outcomes` | First pass / fix / reassign / blocked; learning samples |
| `context_actions` | Context actions and per-candidate estimated costs |
| `usage_events` | Per-turn usage |
| `usage_snapshots` / `provider_health` | Subscription quota snapshots / current Provider state |
| `savings_events` | Savings ledger |
| `provider_profiles` / `model_knowledge_base` / `model_offerings` / `knowledge_blobs` | Definition packs |
| `model_overrides` / `custom_packs` / `learned_stats` / `triage_feedback` / `settings` | User overrides, custom packs, local learning, settings |
| `command_rules` / `approvals` | Command list overrides, approval records |

Full DDL: [schema.md](schema.md).

## 4. Retention and size

- Single `chat_logs.content` cap 64 KB; beyond that keep first 16 KB + last 16 KB and mark truncated; full content remains in the CLI’s own session files.
- `chat_logs` retained 90 days by default; `usage_events`, `savings_events`, `usage_snapshots`, `task_outcomes` 365 days by default; `learned_stats` kept indefinitely.
- Cleanup schedule (F17.b): after the database opens, `setup` starts a low-priority task—first runs `cleanup_retention_records` after ~60 s, then every 24 hours; deletes in batches (5000 rows per batch) then `PRAGMA incremental_vacuum`; results logged with `tracing::info!`. Settings → About provides an “Clean now” button calling the same command.
- Deleting a project removes only Tokage’s own records and the in-project `.tokage/` directory (user confirmation required); does not touch other files or CLI-native session files.

## 5. Privacy

- The database does not store API keys or CLI credentials; proxy credentials live in the system keychain. Commands in logs and approval records are redacted before write (R17).
- Logs may contain code and terminal output; provide “export / clear all records for a project”.

## 6. Frontend sync and legacy import

- On startup, `hydrate()` calls `storage_load` before first render, writing the snapshot into stores; then subscribes to each store, debouncing 300 ms, diffing by reference to find changed rows, and sending them to the writer thread via `storage_write`. Flush immediately on window close and page hide; before app exit the writer thread commits all queued writes.
- Each write item uses its own savepoint in the writer thread; a single failure rolls back only that item. Upserts use `ON CONFLICT DO UPDATE`, not `REPLACE` (which deletes the row first and cascades to child tables).
- Primary keys reuse frontend UUIDs (TEXT). `attempts` temporarily one row per “session × Provider × model”, matching a continued CLI session for the same model; `chat_logs` one row per message, body in `content`, structured fields (reasoning, tool steps, file changes, usage, etc.) in `meta_json`.
- Key-value state in `settings`, keys prefixed: `settings.*` (settings page), `ui.*` (sidebar width, etc.), `chat.*` (model selection and options), `projects.active*`, `knowledge.url`, `providers.lastScan`; `meta.legacyImported` marks legacy data imported. CLI probe results cached in `provider_health.info_json`.
- Legacy import: on first startup if the database lacks `meta.legacyImported` and localStorage has legacy keys (`tokage.projects` / `tokage.chat` / `tokage.settings` / `tokage.providers` / `tokage.ui` / `tokage.knowledge`), read them, run the original store version migrations, then `storage_import` writes everything in one transaction and sets the flag. Import is upsert-only and may be repeated safely.
- Failure handling: if the database cannot open or import fails, Tokage runs on legacy data and keeps writing localStorage in the old format, with a top banner and retry; legacy keys are never deleted. Writes during fallback set `tokage.legacyDirty`; on a later successful startup, re-import once to pick up changes from that period.

## Reliability protocol (m1)

- Queue pipeline snapshots by project/session and copy at enqueue. Increasing revisions reject stale saves; native locking uses unique temporary files, file sync, atomic replacement and Unix directory sync. Failure may occur after replacement; retry confirmation rather than assuming no write.
- Dispatch/approval/acceptance/Git boundaries await persistence. Failure pauses dispatch and exposes retry; close drains writes and stays open on failure. Restore retains DONE, marks in-flight tasks INTERRUPTED and never automatically redispatches.
- Reports/briefs/gates require sessionId + plans.id + taskId, with V12 attempt ownership constraints. Unprovable historical rows remain LEGACY_UNASSOCIATED; never infer identity from T1. Replan clears affected evidence; inherited DONE reports retain an explicit origin-plan pointer.
- Persist scope baselines/checks, failure turnId, gitDeliveries stages and full acceptance content evidence. Rollback bytes live under checkout-private Git tokage-rollback, outside pipeline JSON.
- SQLite, JSON, Git and CLI are not one transaction; in-process save locks exclude neither other applications nor other Tokage processes. Physical power failure was not tested. See [m1a](../reliability-m1a.md), [m1b](../reliability-m1b.md), [m1d](../reliability-m1d.md), [m1e](../reliability-m1e.md), and [m1g acceptance](../reliability-m1g.md).
