# 06 · Usage & Melting Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

1. Record precise per-turn usage: input, output, cache read, cache write, cost.
2. Fetch rolling quota for subscription Providers (utilization, reset time).
3. Plan-level budget: estimate at review, track actual during execution.
4. Maintain a savings ledger so users see how much Tokage actually saved.
5. Melt and recover Providers by quota, budget, and health; supply `scarcity` to the cost model.

## 2. Billing Types

| Type | Examples | Metrics of interest | Price in cost model |
|------|------|---------|----------------|
| `SUBSCRIPTION` | Claude Code (Pro / Max), Codex (ChatGPT subscription), Cursor (subscription), Copilot (Premium request count) | Rolling window utilization, weekly quota, reset time | Equivalent API price × `scarcity` |
| `PAY_AS_YOU_GO` | Aider + DeepSeek / OpenRouter API Key | Tokens, USD | Actual price |

## 3. Per-Turn Usage

At end of each turn the Adapter emits `TurnUsage` ([01 §5](../01-native-environment-engine/README.md)):

- Write to `usage_events`, linked to execution window, task, plan (`ChatRequest.planId` / `taskId` / `domain` / `attemptId`; after plan approval backfill prior `PLANNING` rows with empty `plan_id` via `record_plan_approved`).
- `phase` only `PLANNING` / `EXECUTION` / `REPAIR` / `CONTEXT` / `DIRECT` (separate from `role`; `REVIEWER` / `TESTER` roles default to `EXECUTION`, pipeline dispatch passes `EXECUTION` explicitly, repair turns `REPAIR`).
- Amount: `equiv_cost_usd = (input·p_in + cache_read·p_in·r + cache_write·p_in·w + output·p_out) / 1e6`, price and cache multipliers from effective definition pack candidate; subscription writes `equiv_cost_usd` (keeps CLI `cost_usd`); pay-as-you-go when CLI omits `cost_usd` writes `cost_usd` by same formula.
- When no structured usage, parse trailing stats line; still missing → local estimate (`ESTIMATED`), distinguished on dashboard.
- Also fed to matching engine to update usage profiles ([04 §9](../04-model-matching-engine/README.md)).

## 4. Subscription Quota

### 4.1 Collection (by priority)

1. Passive (default, zero cost): parse quota or rate-limit hints (including 429) in sub-session output immediately; show original text and reset time in red on the corresponding workspace step.
2. Structured sources: read directly when CLI provides quota in structured output or local files (Codex reads rollout log `rate_limits`). Sources requiring CLI config changes (Claude Code statusline input `rate_limits`): Tokage only documents setup steps for user to change; Tokage only watches agreed cache files, display when read, no display when missing.
3. CLI built-in usage commands (default off): CLI runs official usage command, Tokage only reads output, same as user typing in CLI. Commands in table must be verified first; only commands that “do not consume Token / quota” are enabled; command and verification recorded in definition pack `providers.<id>.quota`. Subcommands run as arg array; slash commands via PTY enter CLI → send command → stable frame → regex parse → exit. When enabled, frequency adapts to utilization: < 50% every 30 min, 50–80% every 10 min, > 80% every 3 min; paused when idle.
4. User manual refresh: run items 2 and 3 once immediately.

| CLI | Usage command | Form | Can obtain |
|-----|---------|------|-------|
| Claude Code | `/usage` | Slash (PTY) | 5-hour / weekly / per-model weekly utilization, reset time |
| Codex CLI | `/status` | Slash (PTY) | Primary / weekly window utilization, reset time (skip when item 2 already covers) |
| Antigravity CLI | `/usage` (`/quota`) | Slash (PTY) | Per-model remaining quota, reset time |
| GitHub Copilot CLI | `/usage` | Slash (PTY) | Session usage and quota progress |
| Kiro CLI | `/usage` | Slash (`chat --no-interactive`), whether credits consumed TBD | Credits utilization, reset time |
| Amp | `amp usage` | Subcommand | Balance |
| Augment Auggie | CLI built-in credits query, command TBD | TBD | Credits remaining |
| Reasonix | `/status` | Slash (PTY) | PAYG wallet balance (e.g. `¥29.55`); probe not run by default, on CLI card manual refresh |

Unlisted CLIs (e.g. Cursor CLI) have no built-in usage command; only items 1 and 2.

Tokage does not send quota queries to vendor APIs or read tokens for that ([09](../09-guardrails/README.md) R11); item 3 requests are issued by the CLI.

### 4.2 Local Projection

Between two snapshots, project utilization from equivalent cost accumulated for that Provider:

```
u_now ≈ u_last + Σ equiv_cost_since_last / quota_equiv_cost
```

`quota_equiv_cost` (how many equivalent USD one quota window represents) fitted from utilization delta between adjacent snapshots vs equivalent cost in between, continuously calibrated. Projected values marked “estimated” on dashboard.

Matching engine global allocation adds this plan’s projected consumption on top of `u_now` ([04 §6.1](../04-model-matching-engine/README.md)).

### 4.3 Regex and Time Parsing

Regex shipped in definition pack (`usage_patterns`). Reset time may be relative (“in 2h 13m”) or local time (“3pm”), normalized to UTC; when unparseable keep original text for display.

## 5. Plan Budget and Savings Ledger

### 5.1 Plan Budget

- At review: matching engine per-task estimates, summed to plan estimate, show projected share of each subscription quota.
- During execution: accumulate actual cost per task; orchestration pauses queue and notifies when > 50% over estimate ([03 §3](../03-orchestration-engine/README.md)).
- After completion: estimate vs actual deviation feeds cost calibration coefficient.

### 5.2 Savings Ledger

When a plan reaches delivery terminal state, `settle_plan_savings_cmd` settles into `savings_events`:

| Kind | Baseline (counterfactual) | Actual |
|------|-------------|------|
| `ASSIGNMENT` | Estimated cost if all execution tasks done by “reference model” | Sum of each task’s actual cost |
| `CONTEXT` | Estimated cost of `CONTINUE` / new-window options in context planning | Real cost after action + action cost |
| `DIRECT` | Estimated if direct dispatch tasks done by reference model | Actual cost |

- “Reference model” defaults to highest planning capability among user-installed models, representing “one strong model does everything without Tokage”; changeable in settings. When settling, if `referenceModel ≠ "auto"`, pass that value as `user_override`.
- Before settle, backfill each task’s `tasks.actual_cost_usd` with `SUM(COALESCE(cost_usd, equiv_cost_usd, 0))` from that task’s `usage_events`; plan-level actual same `COALESCE`, so subscription (often only `equiv_cost_usd`) does not zero actual cost.
- Planning cost counts toward actual, not excluded from savings: savings = baseline − (planning + execution + repair). Planning-phase `usage_events` with `plan_id` count naturally.
- Baseline is estimate, clearly marked on dashboard, expandable parameters.
- `DIRECT` / `CONTEXT` events written as planning and context wiring land (F16).

## 6. Melting

### 6.1 Triggers

| Condition | Type | Default threshold |
|------|------|---------|
| Subscription rolling quota utilization | `QUOTA_SOFT` | ≥ 95% |
| CLI returns quota exhausted / rate limit | `QUOTA_HARD` | Immediate |
| Pay-as-you-go daily / monthly spend over budget | `BUDGET` | User setting |
| 3 consecutive timeouts or process errors | `HEALTH` | See [09](../09-guardrails/README.md) |
| Login required | `AUTH` | Immediate |

Between 80–95% no melt; `scarcity = 3.0` steers allocation to other Providers.

### 6.2 Effects

- New plans and reallocation skip this Provider.
- Assigned but not started tasks: matching engine auto-reassigns (task not started, no context loss), changes marked in plan view.
- Running tasks: `QUOTA_SOFT` finish current task; `QUOTA_HARD` / `AUTH` interrupt task, reassign per repair fallback step 2 ([03 §8](../03-orchestration-engine/README.md)), brief includes existing change list.
- Melt state in `provider_health`, `melted_until` from reset time; if unknown re-check in 1 hour.

### 6.3 Recovery

- App-layer bus subscribers handle `TurnFailed` / `TurnSucceeded`: 3 consecutive `Timeout` / `NonZeroExit` / `SpawnFailed` → `HEALTH` melt; `RateLimited` → `QUOTA_HARD` (`melted_until` from quota `reset_at`, else +1h); `AuthRequired` → `AUTH`; success clears consecutive failures, success in half-open restores.
- Rust periodic task (immediately on startup + every 60s) scans expired rows: `QUOTA_*` fetch quota once, recover only if utilization < 80%, else defer 1 hour; `HEALTH` set `HALF_OPEN` (concurrency cap 1).
- `provider_health.status` ownership: provider list snapshot writes must not overwrite `MELTED` / `HALF_OPEN`.
- `meltThreshold` default 0.95 (matches §6.1), used as `QUOTA_SOFT` threshold; with `activeQuotaQuery` on, poll frequency adapts to utilization.
- After recovery, not-started tasks may “reallocate per latest state”; `PAUSED_QUOTA` tasks return to `READY` on `provider-recovered`.

## 7. Dashboard Data

```ts
type UsageDashboard = {
  range: 'today' | '7d' | '30d';
  providers: Array<{
    provider: ProviderId;
    billing: 'SUBSCRIPTION' | 'PAY_AS_YOU_GO';
    usagePct?: number;  usagePctSource?: 'SNAPSHOT' | 'ESTIMATED';
    resetAt?: string;
    spentUsd?: number;  equivCostUsd?: number;
    cacheHitRate: number;
    status: ProviderStatus;  meltReason?: MeltReason;
  }>;
  quality: {
    firstPassRate: number;                                   // first-pass rate across all tasks
    matrix: Array<{ model: string; domain: string; tasks: number; firstPassRate: number }>;
  };
  savings: { totalUsd: number; byKind: Record<'ASSIGNMENT' | 'CONTEXT' | 'DIRECT', number>; referenceModel: string };
  costSplit: { planningUsd: number; executionUsd: number; repairUsd: number };   // planning / execution / repair share
  tokenSeries: Array<{ bucket: string; provider: ProviderId; input: number; cacheRead: number; cacheWrite: number; output: number }>;
};
```

`costSplit` directly reflects whether “expensive models only plan”: high planning share means overweight plans; high repair share means poor matching.
