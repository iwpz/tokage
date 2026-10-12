# 04 · Model Matching Engine

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Responsibilities

This is where Tokage’s core competitive advantage lies: before a task starts, pick the best model for each task.

1. Maintain a model knowledge base: each model’s strengths by domain, pricing, cache characteristics, and context characteristics.
2. Provide a unified `CostModel` that converts any execution plan into equivalent cost.
3. Estimate the one-shot success probability for each model on each task.
4. Perform global allocation for the whole plan: meet the one-shot success threshold, minimize expected total cost, and balance quota and concurrent load.
5. Learn from every task outcome to continuously improve dispatch accuracy.

## 2. Allocation Principles

> Among models with “high enough confidence of getting it right on the first try,” choose the one with the lowest expected total cost.

- Do not use “start with the cheapest, upgrade on failure”: wasted tokens, rework, and diagnosis from a cheap model failing are waste.
- Success probability is a hard gate: models below the threshold do not compete, no matter how cheap.
- Expected total cost includes failure risk: among two models that meet the bar, the one with higher success rate may have lower total cost.
- The value of strong models concentrates on planning and hard tasks; most execution-stage tokens are borne by cost-effective models that meet the bar.

## 3. Domain Expertise

Model capability is scored by domain (1–5); the domain table ships with the definition pack:

| Domain | Description |
|------|------|
| `planning` | Requirement breakdown, API design, architecture constraints (planning role only) |
| `frontend-ui` | Components, styling, interaction, accessibility |
| `backend-api` | Server APIs, business logic, auth |
| `data-model` | Database schema, migrations, queries |
| `algorithm` | Complex algorithms, performance optimization |
| `integration` | Frontend–backend integration, merge conflict handling |
| `refactor` | Large-scale refactors, migrations |
| `infra` | Build, CI, deployment config |
| `testing` | Writing and completing tests |
| `docs` | Documentation |
| `review` | Read-only review |
| `general` | Uncategorized |

Definition pack format: [knowledge-pack.md](knowledge-pack.md).

## 4. Success Probability

For model `m`, task `t` (domain `d`, difficulty `δ ∈ {1,2,3}`, size `σ ∈ {S,M,L}`):

```
Prior:   p₀(m, t) = sigmoid( a × (skill[m][d] − req(δ, σ)) + b )
         req(δ, σ) = requirement score table in the definition pack, e.g. δ=1,S → 2.0; δ=3,L → 4.5
         a, b      = global parameters from the definition pack

Posterior (computed at allocation time):
         `learned_stats` stores **observation weights** `α_obs` / `β_obs` (starting at 0, not prior pseudo-counts)
         FIRST_PASS → α += 1.0; FIXED_IN_WINDOW → α += 0.5, β += 0.5;
         REASSIGNED / BLOCKED / USER_TAKEOVER → β += 1.0
         p(m, t) = calculate_posterior_p(p₀, κ = 6, α_obs, β_obs)
                 = (κ·p₀ + α_obs) / (κ + α_obs + β_obs)
```

- Candidates are `(provider, model)`: the same model through different CLI runtimes may differ in billing, context limits, tool loops, and first-pass rate.
- Local observations aggregate at two levels, key `(provider_name, model_id, domain, bucket = "d<difficulty>", metric)`: each outcome writes two rows—provider-level `(provider, model, …)` and model-level `('*', model, …)`. At allocation time, prefer provider-level when `samples > 0`, else fall back to model-level, else prior `p₀` only.
- “First pass” means passing the acceptance gate within the first execution window ([03 §5](../03-orchestration-engine/README.md)); fixes within the same window count as partial success (0.5).
- User manual reassignment and post-edit outcomes are counted too, as preference signals.
- Setting `confidenceThreshold` overrides definition pack `success_model.p_min.default`; for `difficulty = 3`, use `max(definition pack value, that threshold)`.

## 5. Expected Cost

`CostModel` provides per-window estimated cost `C_exec(m, t)` (§6); expected total cost:

```
E(m, t) = C_exec(m, t)
        + (1 − p(m, t)) × ( C_fix(m, t) + (1 − q) × C_exec(m*, t) )

C_fix   = cost of one in-window fix (≈ 0.3 × C_exec, cache warm)
q       = in-window fix success rate (learned locally, initial 0.5)
m*      = second-best qualifying model for that task (repair fallback step 2 destination)
```

Selection rules:

```
candidates = models that are available (READY / DEGRADED, not melted, support required role permissions)
candidates = candidates.filter(p(m, t) ≥ p_min(t))      // p_min default 0.8; difficulty 3 tasks 0.85
if empty: pick highest p, and mark “no model met confidence threshold” in plan review
chosen = argmin E(m, t)
```

Planning role is handled separately: candidates are models with `planning` skill ≥ definition pack requirement (default 5), sorted by `p` not cost, because planning output is short and quality drives the whole plan.

## 6. CostModel

### 6.1 Effective Price

```
eff_price(m, kind) = price_per_mtok[kind] × scarcity(provider)

scarcity:
  pay-as-you-go = 1.0
  subscription  = tiered by projected utilization u: u < 50% → 0.5; 50–80% → 1.0; 80–95% → 3.0; ≥ 95% → melt
```

`u` is “current utilization + projected consumption from tasks already assigned to this Provider in this plan,” so the more tasks assigned to the same subscription in one plan, the higher its marginal price and tasks naturally spread to other Providers.

### 6.2 Per-Turn Cost

One turn of an agent-style CLI includes multiple API requests (tool loop); each request re-reads the full context:

```
C_turn(X, warm) = P_in × [ X × (warm ? r : w) + (k − 1) × X × r + g × w ] + P_out × o

X = context at turn start (baseline B + history H)
k = API requests this turn; g = new context this turn; o = output this turn
r / w = cache read / write multipliers (both 1 with no cache); warm = whether cache is still hot on first request
```

### 6.3 Single Execution Window

```
C_exec(m, t) = Σ_{i=0}^{n−1} C_turn(B_m + D + i·g, warm_i)

D       = task brief size
n, k, g, o = usage profile by (model, domain, size)
warm_0  = whether baseline is already hot (recent activity on same Provider and shared baseline cache)
warm_i  = between turns CLI runs continuously, usually hot
```

## 7. Global Allocation

Per-task optimum ≠ plan optimum. Allocation walks the full task graph in this order:

1. Traverse tasks in topological order, critical path first.
2. For each task compute `E(m, t)` for each candidate, where:
   - `scarcity` uses projected utilization after cumulative assignments (§6.1).
   - Parallel load: if parallel tasks at the same layer already fill that Provider’s concurrency cap, add wait time to that candidate’s cost (`time_weight × estimated wait`, default weight is small, only matters when costs are close).
   - Window reuse bonus: if upstream and current task share the same model, same domain or adjacent write scope, and no long gap between them, current task may continue upstream’s window ([05 §4](../05-context-lifecycle-engine/README.md)), and `C_exec` is computed for the reuse case.
3. Pick `chosen`, update cumulative utilization and load, continue to next task.
4. Emit per task `Assignment { model, p, est_cost, alternatives[], explanation }`.

Example `explanation`: “frontend-ui · difficulty 2 · size M. Cursor Composer first-pass confidence 0.88, est. $0.21; Sonnet 0.91, est. $0.34 (subscription quota 72%). Chose Cursor Composer.”

Allocation is shown at plan review; user can reassign per task; after reassignment, recompute remaining tasks (cumulative utilization changes).

## 8. Direct Dispatch

The `DIRECT` path ([02 §2](../02-planning-engine/README.md)) uses the same rules for a single task: domain and difficulty from local triage inference, size defaults to `S`.

## 9. Learning

After each task, write `task_outcomes` (driven by repair ladder outcome points) and update `learned_stats`:

| Data | Use |
|------|------|
| First pass / in-window fix / reassignment / blocked / user takeover | Observation weights `success_alpha` / `success_beta` (§4), in-window fix rate `fix_rate` (`q`, initial 0.5) |
| Actual turns, requests per turn, growth, output | Usage profiles (§6.3), `bucket` = task size `S` / `M` / `L`; via `load_layers` → `merge_layers` into `C_exec` |
| Actual vs estimated cost | Cost calibration coefficient `cost_calibration`, key `(provider, model, '*', '*')` |
| User reassignment records | Preference signal: boost prior strength for chosen model next time in same domain |

Read path: `LearnedIndex` built from `list_all_learned_stats`; allocation path (`allocate_plan_models` / `reassign_*` / `assign_direct_task`) calls `posterior_p` / `lookup_fix_rate` when computing `p` and `E`. Settings model cards “prior vs observed” read `list_learned_stats`.

Parameters use exponential moving average (α = 0.2) or observation-weight accumulation; when sample count < 5, blend with definition pack initial values weighted by sample count.

Dashboard shows first-pass rate matrix by “model × domain,” expandable per CLI, so users can see which model is reliable in which domain through which CLI.

## 10. Definition Pack Updates

**Current state (F8 / D6 deferred)**: Official update channel not enabled yet—`OFFICIAL_MINISIGN_PUBKEY` is `None`, Settings auto-check and “Check now” are disabled. Built-in definition pack ships with the installer, works offline; user-configured custom definition pack URLs are unaffected. Definition pack check/download is infrastructure requests, unrelated to R11 “no auth/quota requests to model vendors” ([11 §7](../11-command-authorization/README.md)). Restoring official channel (key, Release assets, `latest.json`, event listeners): [fix-todo F8 deferred items](../fix-todo.md).

Target flow (after channel enabled):

```
Every 24h in background (or user manual):
  GET https://<release-host>/tokage/models/latest.json   // metadata only: pack_version, sha256, url, minisig
  If new version: download → verify minisign signature → verify schema_version → write pending.json → notify frontend
User clicks “Apply”:
  atomic replace → reload knowledge tables in one transaction → next allocation uses new definition
```

- Running plans unaffected; not-yet-started tasks may choose “reallocate per new definition.”
- Signing public key will be embedded (`BuildInfo.knowledgeUpdateEnabled`); installer embeds one definition pack for offline use.
- Update requests only carry app version; local learning data is not cleared on update.
- `knowledgeAutoUpdate` defaults off; forced `false` until channel enabled (settings migration).

## 11. Viewing and Custom Definition Packs

- Built-in, online updates, and user custom definition packs merge in layers; matching reads only the merged view; security-related keys can only be tightened in the custom layer ([knowledge-pack §6](knowledge-pack.md)).
- Built-in definition pack is always viewable read-only in Settings → Model capabilities: domain skills, pricing, context, cache characteristics, usage profiles, compared with local learning data; each field shows source layer ([knowledge-pack §7](knowledge-pack.md)).
- Viewing and customizing are independent: copy from built-in as override or export as custom pack then edit.
- `apply_knowledge_update` replaces the `official` layer, does not affect `custom` or `override` layers.
