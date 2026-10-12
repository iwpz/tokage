# Tokage

**English** | [中文](README.zh-CN.md)

A multi-federation Agent command center and context-lifecycle gateway for independent developers.

Tokage is a cross-platform desktop gateway (Tauri 2 + Rust + React 19). It orchestrates the official AI CLIs already installed on your machine—Claude Code, Codex CLI, Cursor CLI, Antigravity CLI, OpenCode, and others—and turns one request into a pipeline where multiple models collaborate.

## Mission: maximum impact, minimum spend

Tokage exists to **integrate resources so the smallest possible cost produces the largest useful effect**.

“Cost” here is not only money. It includes:

- **Dollar cost** — API bills and subscription burn
- **Token cost** — every prompt and completion that need not be spent
- **Energy cost** — the compute behind those tokens
- **Environmental cost** — fewer wasted tokens means less wasted energy for everyone

If each developer can save some tokens, the aggregate savings scale across the ecosystem. That compounding reduction—in spend, compute, and carbon—is Tokage’s mission, and why we believe the project matters to the open-source AI coding ecosystem: it is infrastructure for doing more with less, without locking you to a single vendor.

## Why the ecosystem needs this

AI coding tools are proliferating. Each CLI is strong in its own lane; none is optimal for every subtask. Without a local orchestrator:

- Strong models burn tokens writing code they need not write
- Cheap models fail and force expensive retries
- Context windows grow until every turn is wasteful
- Developers juggle multiple CLIs by hand and lose the gains of specialization

Tokage closes that gap: **plan with a strong model, execute with the right model, verify locally with zero tokens**. It is vendor-neutral glue for the open tooling stack (including Codex CLI), not a replacement for any one of them.

> [!IMPORTANT]
> **Data and credential boundary (R11)**
>
> Tokage reads only non-secret fields from local CLI config, for UI display and billing-type detection:
>
> | Read | Purpose | Handling |
> |------|---------|----------|
> | Base URL | CLI panel display | Strip userinfo, query, fragment; mask path segments that look like tokens |
> | Auth method, plan, account type | Billing-type detection | Declarative fields only |
> | Key env var name, whether credential file exists | Login-state check | Values never read |
>
> Tokage **does not read** API Key / Token values (discarded if parsed), **does not store, log, send to the UI, or upload them**. It stays vendor-neutral: it **never** carries credentials to call vendor APIs (e.g. it will not use Google auth to query Gemini usage)—usage queries, if any, are issued by the local CLI itself. Definition-pack downloads / update checks and other model-unrelated infrastructure requests are outside this ban (official update channel deferred; see [docs/04 §10](docs/04-model-matching-engine/README.md) / F8). Implementation: [src-tauri/crates/tokage-native/src/usage.rs](src-tauri/crates/tokage-native/src/usage.rs); design: [11 §7](docs/11-command-authorization/README.md).
>
> **These commitments hold only for official releases or builds you compile yourself from [github.com/iwpz/tokage](https://github.com/iwpz/tokage). Do not use installers from unknown sources.** A tampered copy can delete this notice; the app cannot prove it was unmodified—verify against official signatures and release checksums.

## Core strength: pick the right model before you start

Tokage does not save money by “try cheap first, escalate on failure”—a failed cheap run is already waste. It decomposes and assigns **before** execution:

```
User request
   │
   ▼
① Plan: a strong model (e.g. Opus / Fable) reads a project digest and emits a compact design
   │     ─ Interface contracts (APIs, data shapes, component props)
   │     ─ Task graph: N subtasks, dependencies, domains, acceptance criteria
   ▼
② Match: for each subtask, pick the model with acceptable first-pass probability and lowest expected total cost
   │     Frontend tasks → UI-strong models; backend tasks → server-strong models
   ▼
③ Orchestrate: queue by dependency; independent tasks run in parallel
   │     A finishes → report → input for B; everyone shares the same contract
   ▼
④ Verify: run build / typecheck / tests locally (zero tokens)
   │     Repetitive mechanical checks go to inexpensive models when needed
   ▼
Deliver: change summary + what each model did and what it cost
```

Strong models do only their highest-value work (thinking and constraints); their output is deliberately compressed to a few thousand tokens. Most coding and testing tokens go to models that are cheaper and still good enough.

## Token-saving levers

| # | Lever | Module |
|---|-------|--------|
| 1 | Separate plan from execute: expensive models emit a compact design, not implementation code | [02](docs/02-planning-engine/README.md) |
| 2 | Match for first-pass success: assign by domain skill and success probability to avoid rework | [04](docs/04-model-matching-engine/README.md) |
| 3 | Contract constraints: parallel tasks share one contract, cutting integration rework | [02](docs/02-planning-engine/README.md) / [03](docs/03-orchestration-engine/README.md) |
| 4 | Task-scoped windows: each subtask gets a compact brief, not the full chat history | [05](docs/05-context-lifecycle-engine/README.md) |
| 5 | Cache-aware context: continue, compact, or open a new window per provider cache behavior | [05](docs/05-context-lifecycle-engine/README.md) |
| 6 | Local zero-token mechanisms: project digest, triage, acceptance gates, usage collection stay on-device | [01](docs/01-native-environment-engine/README.md) / [03](docs/03-orchestration-engine/README.md) |
| 7 | Continuous learning: record first-pass rates by domain so the next assignment is sharper | [04](docs/04-model-matching-engine/README.md) |

## Core principles

- **Official first**: call only native commands of official CLIs; no shell aliases injected into the user’s Shell, no edits to rc files or CLI config.
- **Structured first**: prefer headless structured CLI output; PTY screen scraping is fallback only.
- **Local first**: data lives in on-device SQLite; no auth/quota calls to model vendors. Definition-pack download and update checks are infrastructure, unrelated to model calls.
- **Restricted by default**: model-initiated commands follow an authorization mode (allow or ask); execution windows run in a system-level sandbox and cannot read credentials.
- **Trust the source**: the commitments above hold only for official releases or self-builds from [github.com/iwpz/tokage](https://github.com/iwpz/tokage).
- **Explainable and interruptible**: planning, assignment, and context actions carry estimated cost and rationale; plans can be reviewed and edited before execution.

## Design docs

| # | Module | Summary |
|---|--------|---------|
| 00 | [Architecture overview](docs/00-architecture-overview/README.md) | Stack, process model, core loop, crate layout, IPC contract |
| 01 | [Native environment & federation](docs/01-native-environment-engine/README.md) | CLI discovery, Provider Adapter, headless / PTY, role permissions |
| 02 | [Planning engine](docs/02-planning-engine/README.md) | Triage, project digest, strong-model planning, design + contracts, task graph |
| 03 | [Orchestration engine](docs/03-orchestration-engine/README.md) | Queue & DAG scheduling, artifact passing, acceptance gates, repair fallback |
| 04 | [Model matching engine](docs/04-model-matching-engine/README.md) | Knowledge pack, domain skill, success probability, expected cost, global assignment |
| 05 | [Context lifecycle engine](docs/05-context-lifecycle-engine/README.md) | Task briefs, window reuse, cache-aware continue / compact / new-window |
| 06 | [Usage & melting engine](docs/06-usage-melting-engine/README.md) | Subscription quotas, metered billing, budgets, savings ledger, melt & recovery |
| 07 | [Session aggregation tree](docs/07-session-aggregation-engine/README.md) | Main session → plan → task → attempt tree, events, human intervention |
| 08 | [Local storage](docs/08-storage/README.md) | SQLite schema, indexes, migrations, retention |
| 09 | [Guardrails](docs/09-guardrails/README.md) | Output filters, timeouts, process-group cleanup, write scope, command safety |
| 10 | [Frontend Mission Control](docs/10-frontend/README.md) | Plan review, pipeline board, session view, usage & savings |
| 11 | [Command authorization & credential isolation](docs/11-command-authorization/README.md) | Auth modes, allowlists, sandbox, credential isolation, source trust |
| 12 | [Network proxy](docs/12-network-proxy/README.md) | Per-CLI proxy modes, local CONNECT proxy, forced egress |

Each module directory’s `README.md` is the design source of truth; finer formats, templates, and assets live alongside it.

Suggested reading order: 00 → 02 → 04 → 03 → 05 → 01 → 06; others as needed.

Roadmap: [docs/TODO.md](docs/TODO.md). Fix checklist: [docs/fix-todo.md](docs/fix-todo.md).

## Development & testing

Local (network required for deps; ask the user before any `npm ci` / `cargo fetch`):

```bash
npm ci
npm run typecheck
npm test

cd src-tauri && cargo test --workspace --locked
```

Tests that need a real local CLI are marked `#[ignore]` (`chat::live::*`, `providers::live::*`) and are skipped by default. For manual acceptance:

```bash
cd src-tauri && cargo test --lib chat::live -- --ignored --nocapture
```

GitHub Actions CI is not enabled yet; run the commands above locally.

IPC wiring check (see [docs/fix-todo.md](docs/fix-todo.md) appendix F): lists production TypeScript callers missing for `#[tauri::command]`s; commands not on the allowlist exit with code 1.

```bash
python3 -I scripts/check-ipc-wiring.py
# Allowlist: scripts/ipc-wiring-allowlist.txt (each line: command_name  reason)
```

## Glossary

| Term | Meaning |
|------|---------|
| Provider | A local official CLI and its vendor account (e.g. a Claude Code subscription) |
| Main Session | One request line the user sees in the sidebar |
| Plan | Design + contracts + task graph from the planning model; may have multiple versions |
| Contract | Interface definitions that constrain tasks: APIs, data shapes, component props, directory conventions |
| Task | A subtask in a plan, with domain, dependencies, write scope, and acceptance criteria |
| Attempt | One real execution of a task on a CLI (one CLI session) |
| Task Brief | First input to an attempt: task card + relevant contracts + upstream reports |
| Task Report | Compact result when a task finishes; passed to downstream tasks |
| Gate | Build / typecheck / test commands Tokage runs locally after a task |
| First-pass rate | Share of tasks that pass the gate inside the first attempt; measures assignment accuracy |
| Equivalent cost | Normalized cost: metered = real USD; subscription = equivalent API price × quota scarcity factor |
| Melt | Temporarily remove a Provider from assignment when usage or health crosses a red line |
