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

### 4.1 Collection and separate consent

Local history, normal task quota hints and user-configured statusline caches are read by default. History does not establish current-account quota. `TurnUsage` accounting stays separate from account snapshots; online results never become task spending events.

Installation-local “Allow online usage queries” permission enables automatic queries for enabled supported CLIs. Scans revalidate sources without clearing all history. Success TTL is 10 minutes, with at least 60 seconds between automatic attempts; expiry and failure backoff determine scheduling. HTTP 429 respects Retry-After; authentication rejection prevents repeated automatic requests. Manual refresh shares permission, identity and backoff gates. Revocation cancels queues and in-flight calls while preserving original receipt times. It authorizes no inference or CLI account changes.

- Claude: official subscription CLI `/usage`, preserving reported windows.
- Cursor: interactive `/usage` in an already trusted directory, gated by recognized built-in completion; allocations stay separate and date labels stay literal.
- Copilot: non-inference management RPC quotas with vendor-reported units, not session spending.
- Antigravity: fixed non-inference management command with model/group/bucket scopes.
- OpenCode V2 and Reasonix: each CLI's connected official DeepSeek wallet through read-only `https://api.deepseek.com/user/balance`; no attestation of arbitrary session model billing.
- Kiro: fixed official account GET for estimated credits; rejected authentication may delegate refresh to the fixed CLI management command. See [Kiro validation](../kiro-usage-validation.md).
- Codex: online production transport remains disabled; local history and offline protocol decoder only. Aider and detection-only CLIs do not claim online quota support.

Queries require current source binding. Unknown structures or unsupported authentication remain explicit states, never fabricated zeros, tokens, dollars or reset windows. Only current valid account-scoped quotas can drive provider-wide soft melting; credits/wallet/model scopes do not imply general limits. See [support audit](../cli-usage-support-audit.md) for version, auth and validation boundaries.

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
