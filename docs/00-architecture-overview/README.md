# 00 · Architecture Overview

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Goals and Non-Goals

Goals:

- A single desktop entry point that orchestrates officially installed AI CLIs on the machine.
- Plan first, then dispatch: a strong model produces a concise design and task graph; each task goes to the best-suited model for execution.
- Tasks queue by dependency, run in parallel where allowed; artifacts flow along the dependency chain under unified contract constraints.
- Manage each execution window’s context in a cache-aware way.
- Present subscription quotas, usage-based billing, first-pass rate, and savings in one place, and use them for melting and reallocation.

Non-goals (v1):

- Do not decrypt, read, or rewrite vendor API requests; no HTTP man-in-the-middle; an optional local proxy only does CONNECT forwarding ([12](../12-network-proxy/README.md)).
- Do not intercept CLI outbound destinations per vendor.
- Do not host user API keys; credentials are managed by each CLI.
- No multi-machine sync or team collaboration.
- Do not reinvent agent capabilities; file I/O and tool execution are done by each CLI.

## 2. Tech Stack

| Layer | Choice | Notes |
|----|------|------|
| Desktop shell | Tauri 2.x | Small binary and resident memory; capability model minimizes frontend permissions |
| Frontend | React 19 + TypeScript + Vite | |
| Styling / components | Tailwind CSS 4 + in-house `src/components/ui.tsx` | shadcn/ui not adopted; switching is on the roadmap |
| Frontend state | Zustand | TanStack Query not adopted; see [10](../10-frontend/README.md) |
| Backend runtime | Rust + Tokio (multi-thread) | |
| Subprocesses | `tokio::process` (headless) / `portable-pty` (fallback) | See [01](../01-native-environment-engine/README.md) |
| ANSI parsing | `vte` + `strip-ansi-escapes` | |
| Code structure parsing | Line-prefix symbol extraction on first 300 lines per file | tree-sitter not adopted; project digest generation see [02](../02-planning-engine/README.md) |
| Storage | SQLite 3 + `rusqlite` (bundled) + `refinery` | WAL, single writer thread + read connection pool |
| Token estimation | byte count ÷ 4 (heuristic) | tiktoken-rs not adopted; switching is on the roadmap |
| Logging | `tracing` + `tracing-appender` | |

## 3. Core Loop

```
            ┌────────────────────────────────────────────────────────┐
            │                      User request                       │
            └───────────────────────────┬────────────────────────────┘
                                        ▼
                         ┌──────────── Triage (local) ────────────┐
                         │ Simple → dispatch single task directly │
                         │ Complex → enter planning               │
                         └──────┬──────────────────────┬──────────
                                ▼                      │
                 PlanningEngine: strong model + project digest       │
                 → design + contracts + task graph (concise)         │
                                │                      │
                          User review / edit            │
                                ▼                      ▼
                 MatchingEngine: pick model per task (success × cost × load)
                                ▼
                 OrchestrationEngine: DAG queue → execute → report → downstream
                                │          ▲
                                ▼          │ brief / window reuse
                 ContextEngine ────────────┘
                                ▼
                 Acceptance gate (local commands) → pass / repair fallback
                                ▼
                 UsageEngine: real usage → learn first-pass rate & usage profile → better matching next time
```

## 4. Process Model

```
┌──────────────────────── Tokage process ─────────────────────────┐
│  WebView (React)                                                │
│     │  invoke() / Channel<UiEvent>                              │
│  ───┼────────────────────────────────────────────────────────── │
│  Tauri Core (Rust, Tokio)                                       │
│     ├─ Orchestrator composition layer                           │
│     ├─ PlanningEngine     ├─ MatchingEngine (incl. CostModel)   │
│     ├─ OrchestrationEngine├─ ContextEngine                      │
│     ├─ NativeEngine ── ProviderAdapter × N                        │
│     ├─ UsageEngine        ├─ SessionEngine (events → frontend)  │
│     ├─ AuthEngine (auth / sandbox / credential isolation)├─ ProxyRelay (optional) │
│     └─ Storage                                                  │
└──────────────┬──────────────────────────────────────────────────┘
               │ spawn (each child in its own process group)
     ┌─────────┼──────────┬───────────┐
     ▼         ▼          ▼           ▼
  claude     codex      cursor-agent  agy ...  ← official CLIs unchanged
```

- Child process working directory is project root (or task-specific worktree; see [03 §6](../03-orchestration-engine/README.md)).
- Child processes inherit the logged-in shell environment; no extra business env vars injected.
- Each child runs in its own process group; on timeout or cancel the whole group is reaped ([09](../09-guardrails/README.md)).

## 5. Rust Crate Layout

```
src-tauri/
  crates/
    tokage-core/           # Domain models, errors, event definitions (no IO)
    tokage-native/         # Provider detection, Adapter, PTY, usage read, proxy probe
    tokage-planning/       # Triage, project digest, planning calls, plan parse & validate
    tokage-matching/       # Definition pack, CostModel, success probability, task assignment, learning
    tokage-orchestration/  # DAG scheduling, queue, acceptance gates, repair strategy, session tree types
    tokage-context/        # Task brief, window reuse, context action planning
    tokage-storage/        # SQLite access layer & migrations (incl. savings ledger, melt state)
  src/                     # Tauri entry, command registration, policy / knowledge / dashboard
```

> Current state (2026-10-08): **7 crates** as above. No `tokage-usage` / `tokage-session` / `tokage-auth` / `tokage-proxy` crates—usage and melting live in `tokage-native` + `tokage-storage` + app-layer subscribers; session tree types in `tokage-orchestration::tree`; unified approval and minimal `UiEvent` batching wired on the frontend (full `set_focus` / reconnect not end-to-end); command authorization and sandbox in `tokage-core` + `tokage-native::sandbox` + `src/policy.rs`; proxy in `tokage-native::proxy` + app-layer commands.

All crates depend on `core`; engines communicate via trait objects and an event bus injected by the composition layer for easy test doubles. `CostModel` is implemented in `tokage-matching` and shared by planning (planner model pick), matching, and context.

## 6. Internal Event Bus

```rust
// tokage_core::DomainEvent — source of truth in code (2026-10-08)
pub enum DomainEvent {
    ProviderDetected { provider: ProviderId, status: ProviderStatus },
    ProviderMelted { provider: ProviderId, reason: String },
    ChatOutput { turn_id: String, event: EventKind },
    ChatEnded { turn_id: String, cancelled: bool },          // F2: cancelled == true on cancel
    TurnFailed {                                             // F15
        provider: ProviderId,
        kind: TurnErrorKind,
        message: String,
        melted_until: Option<String>,
    },
    TurnSucceeded { provider: ProviderId },                  // F15
    TriageDone { main_id: Option<i64>, route: TriageRoute },
    TaskStateChanged {
        plan_id: String,
        task_id: String,
        old_state: String,
        new_state: String,
        reason: Option<String>,
    },
    KnowledgePackUpdateAvailable { pack_version: String, pending_path: String },
}
```

> Current state (2026-10-08, F18): `tokage_core::Bus` guarantees each subscriber receives all events in publish order; dropped subscribers are removed on the next publish. App-layer forwarding: `ChatOutput` → `chat-event`; `ChatEnded` → `chat-ended` (`{ turnId, cancelled }`); `TurnFailed` / `ProviderMelted` / recovery → `provider-melted` / `provider-recovered` (F15). Design events such as `PlanDrafted` / `AttemptStarted` listed in older docs are **not** implemented as `DomainEvent` variants yet.

## 7. IPC Contract (Tauri Commands)

Parameters and return values are `serde`-serialized; TypeScript types are hand-written in `src/lib/ipc.ts`, one-to-one with Rust definitions. `specta` / `tauri-specta` not used for now: Tauri 2.0 has been RC since 2023-07 with breaking changes between RCs and must stay in sync with specta versions Tauri depends on; `u64` / `i64` fields and `serde_json::Value` need per-field annotations or remapping; `tauri-specta` wraps returns as `{ status, data | error }`, requiring changes at every call site. Re-evaluate after 2.0 stable release.

> Current state (2026-10-08, F18): Registered commands follow `src-tauri/src/lib.rs` `invoke_handler` and `src/lib/ipc.ts` (~119; `scripts/check-ipc-wiring.py` validates production callers). The table below is **representative** production commands; see code for the full list. F17.c removed unused commands (e.g. `classify_command`, `get_builtin_knowledge_pack`, `list_usage_events`, `list_savings_events_cmd`, `get_provider_proxy_status`, `record_provider_failure_cmd`, `reset_provider_failures_cmd`, etc.). `evaluate_window_reuse_cmd` exemption deferred.

| Command | Purpose |
|---------|------|
| `refresh_providers` / `refresh_provider` / `reload_shell_env` | Provider detection & environment |
| `start_chat_turn` / `cancel_chat_turn` | Chat turns (events via `chat-event` / `chat-ended`) |
| `triage_request` / `render_planning_prompt` / `get_repo_digest` / `validate_plan` / `persist_plan` / `load_plan` | Planning |
| `assemble_replan_prompt` / `compute_plan_diff` / `apply_plan_diff` / `generate_adaptation_tasks` | Incremental replanning |
| `allocate_plan_models` / `assign_direct_task` / `reassign_task` | Model assignment |
| `pick_dispatchable_tasks` / `execute_gate_command` / `cancel_gate_command` | Scheduling & acceptance |
| `decide_repair_action_cmd` / `generate_implementation_fix_task_cmd` | Repair ladder |
| `plan_in_window_context_cmd` / `apply_context_action_cmd` / `build_compact_cmd` / `triage_main_session_message_cmd` | Context |
| `trip_provider_melt_cmd` / `recover_provider_health_cmd` / `list_provider_health_cmd` | Melting |
| `get_usage_dashboard` / `settle_plan_savings_cmd` / `record_savings_event_cmd` | Usage & savings |
| `list_native_sessions` / `cleanup_retention_records_cmd` | External sessions / retention cleanup |
| `authorize_command` / `resolve_approval` / `get_command_policy` | Command authorization |
| `get_all_provider_proxy_statuses` / `test_proxy_connection` | Network proxy |
| `build_info` | Version & build fingerprint |

## 8. Security and Privacy

- Tauri capabilities expose only Tokage’s own commands, not blanket `shell` / `fs` plugin permissions to the frontend.
- Subprocess commands are built only by backend Adapters; user input goes via `Command::arg` or stdin, never through `sh -c`.
- Acceptance commands in plans are visible when the user reviews the plan and must match the allowlist ([09 §5](../09-guardrails/README.md)).
- The display layer receives sanitized config, identity sources and usage; CLI credentials needed for online queries stay in native memory, never persisted or sent to the frontend (R11, [11 §7](../11-command-authorization/README.md)).
- Model-initiated commands go through authorization mode and command lists; execution-role CLIs run in OS-level sandbox without credential paths or other vendors’ keys ([11](../11-command-authorization/README.md)).
- Dedicated online quota queries are off by default. Installation-local permission enables non-inference CLI management commands or fixed read-only vendor endpoints. Revocation cancels scheduling; CLI tools still manage accounts and refresh. Pack downloads and update checks are separate infrastructure.

## 9. Cross-Platform

| Item | macOS / Linux | Windows |
|----|---------------|---------|
| Login shell environment | One-shot `$SHELL -ilc 'env -0'` | Inherit process env + registry PATH |
| Process group cleanup | `setsid` + `killpg(SIGTERM → SIGKILL)` | Job Object `KILL_ON_JOB_CLOSE` |
| OS-level sandbox | macOS `sandbox-exec`; Linux Landlock / bubblewrap | Not in v1 |
| System proxy read | SystemConfiguration; desktop settings / env vars | Registry / WinHTTP |
| PTY | `portable-pty` (openpty) | ConPTY (Win10 1809+) |
| CLI executables | `which` crate | Also match `.cmd` / `.exe` |
