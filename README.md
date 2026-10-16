# Tokage

Reliability scope: [M2 targets](docs/reliability-m2a.md). Current natural-language desktop results and pending targets are recorded in [m2b evidence](docs/reliability-m2b.md). Failure/recovery scenarios and daily reliability remain unverified; CLI readiness is not certification.

**English** | [中文](README.zh-CN.md)

A local desktop gateway that orchestrates the official AI coding CLIs on your machine—Claude Code, Codex CLI, Cursor CLI, OpenCode, and others—so one request becomes a multi-model pipeline that **does more work with fewer tokens**.

Built with Tauri 2, Rust, and React. Vendor-neutral: Tokage does not replace your CLIs; it coordinates them.

<p align="center">
  <img src="docs/images/chat-generated-plan.png" alt="Tokage chat with a generated plan" width="800" />
</p>

## Mission

Integrate what you already have so the **smallest spend produces the largest useful effect**.

Spend includes more than money:

- **Money** — API bills and subscription burn
- **Tokens** — every prompt and completion that did not need to be sent
- **Energy** — the compute behind those tokens
- **Environment** — less wasted compute for everyone who uses AI coding tools

If each developer saves some tokens, the aggregate savings compound across the ecosystem. That is Tokage’s job: infrastructure for doing more with less, without locking you to a single vendor.

## How it works

Tokage does not “try the cheap model first and escalate on failure.” A failed cheap run is already waste. It **plans once with a strong model, then assigns each subtask to a capable, cost-effective model**.

```
You describe a goal
        │
        ▼
┌───────────────────────┐
│  Model management     │  Discover installed official CLIs,
│                       │  sync available models, track usage
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│  Model knowledge base │  Domain skills, prices, cache traits,
│                       │  success priors + local learning scores
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│  Strong model plans   │  Compact design, contracts, task graph
│                       │  (thinking & constraints only—short output)
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│  Smaller models work  │  Each task → best first-pass / cost fit
│                       │  Parallel where independent; shared contracts
└───────────┬───────────┘
            ▼
┌───────────────────────┐
│  Local verification   │  Build / typecheck / tests on your machine
│                       │  (zero tokens)
└───────────────────────┘
```

You can review and edit the plan before anything runs. Strong models spend tokens where they matter most; most coding tokens go to models that are cheaper and still good enough for the task.

### Plan review

Before work starts, Tokage surfaces a structured plan you can edit: goal and scope, shared contracts, task graph with model routing, then a confirm step with cost and parallelism.

| Spec | Contracts |
|:----:|:---------:|
| <img src="docs/images/plan-spec.png" alt="Specification review: goal, scope, and key decisions" width="400" /> | <img src="docs/images/plan-contracts.png" alt="Shared contracts that all tasks must satisfy" width="400" /> |

| Tasks & model assignment | Confirm & run |
|:------------------------:|:-------------:|
| <img src="docs/images/plan-tasks-models.png" alt="Tasks with model and CLI assignment, pass rate, and cost" width="400" /> | <img src="docs/images/confirm-execution.png" alt="Confirm parallel pipeline execution with dependencies" width="400" /> |

## Model knowledge base & scoring

Tokage ships a **model definition pack** (built-in, optionally updated online, overridable with your own pack). It is the shared database behind matching:

| What it stores | Why it matters |
|----------------|----------------|
| Domain skill scores (1–5) | frontend, backend, planning, testing, … |
| Pricing & billing type | metered vs subscription equivalent cost |
| Context & cache traits | so context strategy matches each CLI |
| Role support | planner / executor / reviewer |

**Matching rule:** among models with a high enough chance of getting the task right on the **first pass**, pick the one with the **lowest expected total cost** (including repair risk).

Scores combine:

1. **Prior** — skill vs task difficulty/size from the definition pack  
2. **Posterior** — your local outcomes (first-pass, fixed in-window, reassigned, …) update that prior over time  

So the system starts from curated knowledge and gets sharper on *your* projects. Planning roles prefer the strongest planners; execution prefers cost-effective models that clear the confidence bar.

## Security commitments

Tokage is a **local orchestrator**, not a proxy for vendor APIs.

- **Never** sends CLI / model-vendor requests on your behalf—no auth, no quota polling, no usage queries using your credentials. Those stay with the official CLIs you already installed.
- **Never** reads, stores, logs, or uploads API keys / tokens. At most it reads non-secret config fields (e.g. Base URL for display, plan type for billing UI)—key values are discarded if ever parsed.
- **Only** intentional outbound network use by Tokage itself: **check for updates** and **download model definition packs** (infrastructure, unrelated to your chat or CLI sessions).

Run official releases or builds you compile from this repository. Prefer verifying release signatures and checksums; a tampered build can claim anything.

## Documentation

Design docs (English default, Chinese via each page’s language switcher):

- [Architecture overview](docs/00-architecture-overview/README.md)
- [Planning](docs/02-planning-engine/README.md) · [Matching & knowledge pack](docs/04-model-matching-engine/README.md) · [Orchestration](docs/03-orchestration-engine/README.md)
- [Context](docs/05-context-lifecycle-engine/README.md) · [Usage](docs/06-usage-melting-engine/README.md) · [Command authorization](docs/11-command-authorization/README.md)

Full index: see the module list under `docs/`.

## Build from source

```bash
npm ci
npm run typecheck
npm test
cd src-tauri && cargo test --workspace --locked
```

Requires the official AI CLIs you want to orchestrate to be installed separately on your machine.
