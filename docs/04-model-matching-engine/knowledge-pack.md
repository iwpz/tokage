# Model Definition Pack Format

**English** | [中文](knowledge-pack.zh-CN.md)

Back to [04 · Model Matching Engine](README.md)

Definition packs are published by Tokage maintainers, ship built into the app, and can be updated online. All pricing, cache, and score values are definition pack fields, maintained from vendor official docs and benchmarks; numbers below are structural examples only.

## 1. Top-Level Structure

```json
{
  "schema_version": 1,
  "pack_version": "2026.10.01",
  "domains": ["planning", "frontend-ui", "backend-api", "data-model", "algorithm",
              "integration", "refactor", "infra", "testing", "docs", "review", "general"],
  "success_model": { "a": 1.6, "b": 1.2, "prior_strength": 6,
                     "req": { "1": { "S": 2.0, "M": 2.5, "L": 3.0 },
                              "2": { "S": 3.0, "M": 3.5, "L": 4.0 },
                              "3": { "S": 4.0, "M": 4.3, "L": 4.5 } } },
  "providers": { },
  "models": { },
  "usage_profiles": { },
  "triage_rules": [ ],
  "usage_patterns": { },
  "noise_rules": [ ],
  "gate_allowlist": [ ]
}
```

## 2. providers

```json
"CLAUDE_CODE": {
  "billing": "SUBSCRIPTION",
  "cache": {
    "mode": "explicit_prefix",
    "ttl_seconds": 300,
    "read_multiplier": 0.1,
    "write_multiplier": 1.25,
    "shared_baseline": true
  },
  "context": { "cli_auto_compact_ratio": 0.92, "compact": "native", "compact_output_ratio": 0.06 },
  "concurrency_hint": 2,
  "models": [
    { "model": "claude-opus-5-5", "cli_model_arg": "opus" },
    { "model": "claude-haiku-4-5", "cli_model_arg": "haiku" }
  ]
}
```

`providers.<id>.models` lists models offered by that CLI; each entry only fields that vary by channel:

| Field | Description |
|------|------|
| `model` | Key referencing `models` |
| `cli_model_arg` | Literal string passed to CLI `--model` |
| `context_window` / `effective_context` | Optional CLI context limits (e.g. Codex 272K), must not exceed model’s own values |
| `credit_multiplier` | Optional consumption multiplier vs baseline for credit-based CLIs |
| `cli_model_arg_verified` | When `false`, arg not verified against real CLI catalog; Adapter uses CLI-returned model list |
| `skills` | Optional per-channel domain score overrides, overriding model values by domain |

| `cache.mode` | Meaning |
|--------------|------|
| `explicit_prefix` | CLI sets cache breakpoints explicitly, stable hits, usually write premium, shorter TTL |
| `automatic_prefix` | Server auto prefix cache, no write premium |
| `implicit` | Implicit cache, hits not guaranteed; use with `hit_reliability` |
| `disk_persistent` | Disk-level cache, retained for hours+ |
| `none` | No cache |
| `by_model` | Determined by underlying model (multi-model CLI); read `cache` on model entry |

## 3. models

Keyed by model ID, channel-agnostic intrinsic model data. When the same model is offered by multiple CLIs, write once; channel differences go in `providers.<id>.models` (§2).

```json
"claude-opus-5-5": {
  "display_name": "Claude Opus 5.5",
  "vendor": "anthropic",
  "roles": ["planner", "executor", "reviewer"],
  "skills": {
    "planning": 5, "frontend-ui": 4, "backend-api": 5, "data-model": 5, "algorithm": 5,
    "integration": 5, "refactor": 5, "infra": 4, "testing": 4, "docs": 4, "review": 5, "general": 5
  },
  "price_per_mtok": { "input": 0.0, "output": 0.0 },
  "context_window": 200000,
  "effective_context": 120000,
  "speed": 2,
  "tags": ["high-reasoning", "architecture"],
  "description": "Suited for planning, architecture constraints, and hard tasks."
}
```

| Field | Description |
|------|------|
| `roles` | Roles the model can hold; `planner` only granted to top tier of planning skill |
| `skills` | 1–5 per domain, main input to success probability prior |
| `price_per_mtok` | Official API price; subscription Providers use this as equivalent price |
| `context_window` / `effective_context` | Model’s own context window and where quality drops noticeably; hard constraints for context planning |
| `cache` / `context` | Optional overrides of Provider-level defaults |
| `cache_rates` | Optional read/write multipliers for this model under `by_model` Provider |
| `long_context_pricing` | Optional `{threshold, input_multiplier, output_multiplier}`: whole request billed at multipliers when input exceeds threshold |
| `price_schedule` | Optional time-of-day pricing (e.g. peak multiplier and UTC window) |
| `max_output` / `released` / `vendor` | Reference info, not used in matching |
| `benchmarks` | Reference scores, display and manual verification only, not used in computation |
| `price_estimated` / `skills_estimated` | Price or domain scores estimated when no official data; UI marks “estimated” |

The merged view expands each `(provider, model)` into one candidate: model entry as base, channel entry overrides field by field. Matching engine reads only expanded candidates.

## 4. usage_profiles

Initial usage profile per execution window by `(domain, size)`; gradually replaced by local learning:

```json
"usage_profiles": {
  "frontend-ui": { "S": { "turns": 1, "calls_per_turn": 8,  "growth_per_turn": 10000, "output_per_turn": 3000 },
                   "M": { "turns": 2, "calls_per_turn": 14, "growth_per_turn": 18000, "output_per_turn": 6000 },
                   "L": { "turns": 3, "calls_per_turn": 20, "growth_per_turn": 25000, "output_per_turn": 9000 } },
  "testing":     { "S": { "turns": 1, "calls_per_turn": 6,  "growth_per_turn": 8000,  "output_per_turn": 3000 } }
}
```

## 5. Other Rule Tables

| Key | Purpose | Consumer |
|----|------|-------|
| `triage_rules` | Triage keywords, regex, weights | [02 §2](../02-planning-engine/README.md) |
| `domain_keywords` | Domain inference keywords (multi-domain triage and direct dispatch domain inference) | [02 §2](../02-planning-engine/README.md) |
| `usage_patterns` | Regex to parse subscription quota text, grouped by Provider, `_generic` for all | [06 §4](../06-usage-melting-engine/README.md) |
| `time_formats` | Relative time / local time regex for reset times | [06 §4.3](../06-usage-melting-engine/README.md) |
| `providers.<id>.quota` | Quota window type and free query commands | [06 §4](../06-usage-melting-engine/README.md) |
| `noise_rules` | Output noise filter rules | [09 §1](../09-guardrails/README.md) |
| `gate_allowlist` | Acceptance command allowlist patterns (`check_gate_command_safety` / `validate_plan` / sole source for execution gates; commands not on list and not `confirmedCommands` rejected) | [09 §5](../09-guardrails/README.md), [03 §5](../03-orchestration-engine/README.md) |
| `command_policy` / `forbidden_paths` | Command allowlist tiers, credential paths | [11 §3](../11-command-authorization/README.md) |
| `credential_env` / `providers.<id>.env_allow` | Credential variable trimming | [11 §6.3](../11-command-authorization/README.md) |
| `redaction_patterns` | Common key patterns for output redaction | [11 §6.4](../11-command-authorization/README.md) |
| `sandbox_profiles` | Sandbox profile per CLI | [11 §5.2](../11-command-authorization/README.md) |
| `providers.<id>.proxy` | Proxy detection cases | [12 §2](../12-network-proxy/README.md) |

## 6. Layering and Local Overrides

Definition packs merge by layer; higher layers override lower:

| Layer | Source | Editable |
|----|------|-------|
| `builtin` | Shipped with installer | No |
| `official` | Online update ([04 §10](README.md)), replaces `builtin` when present | No |
| `custom` | User-imported custom definition packs, multiple, merged in order | Import / remove / reorder |
| `override` | Per-field overrides in settings, stored in `model_overrides` (JSON Merge Patch) | Yes |
| `learned` | Local learning (success posterior, usage profiles, calibration) | Reset only |

- Custom definition packs use the same schema; may include partial models or fields; no signature required, but schema validated on import and UI marks “unsigned.”
- **Persistence (F14)**: Multiple custom packs written to SQLite `custom_packs` (`position` / `enabled` / `name` / `pack_json`), `replace_custom_packs` replaces table in order; `hydrateFromDb` on startup. `model_overrides` persisted and restored via IPC `upsert_model_override` / `delete_model_override` / `list_model_overrides`. Rust `load_layers(db)` reads enabled customs + overrides + usage profile metrics for allocation and dashboard `get_effective_pack`.
- Security keys (`command_policy.forbidden`, `forbidden_paths`, `credential_env`, `sandbox_profiles`) in `custom` / `override` layers can only append or tighten, not delete or relax.
- Custom layer overriding `models.<id>` affects all channels; overriding entry in `providers.<pid>.models` affects only that channel, matched by `model`.
- Merged result `effective` is the only view the matching engine reads.

## 7. Viewing

Settings → Model capabilities; built-in and custom definition packs use the same view:

- Layer picker: `effective` (default) / `builtin` / `official` / each `custom` pack; `builtin` and `official` read-only, always viewable.
- Model list: one row per model showing roles, price, context window / effective context, speed, tags, description, and CLIs offering the model; expand for per-CLI model args, context limits, billing, verification status.
- Model detail: domain skill radar or table (1–5), cache characteristics, `usage_profiles`; each field shows source layer (e.g. “builtin · overridden by custom:my-pack”), expandable per-layer raw values.
- Learning data side by side: local first-pass rate and sample count per domain vs prior scores.
- Rule tables (domains, triage rules, command policy, sandbox profiles, proxy detection) as read-only tables.
- From any layer “copy as override” or “export as custom pack” as edit starting point; viewing does not modify any layer.
