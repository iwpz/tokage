I'll review the existing code before writing the plan.

# Spec
## Goals
Implement three pure ES-module functions — `subtotal` in `pricing.mjs`, `money` in `format.mjs`, and `invoice` in `invoice.mjs` — so they satisfy the fixed assertions in `check.mjs`. Pricing and formatting are independent and must be built in parallel; `invoice` then composes both.

## Scope
- In scope: editing `pricing.mjs`, `format.mjs`, `invoice.mjs` only; preserving public named exports.
- Out of scope: tests (`check.mjs`), package files, configs, new dependencies, new files, shell execution by agents.

## Key decisions
- Decision: `subtotal` and `money` are separate parallel tasks because neither imports the other. Rationale: maximal safe parallelism.
- Decision: `invoice` is a dependent integration task importing both public functions. Rationale: it can only run after both land.
- Decision: validation lives in `subtotal` and propagates through `invoice` unchanged. Rationale: single source of truth for `RangeError`.
- Decision: money formatting avoids `Intl`/`toLocaleString` (thousands separators). Rationale: required exact two-digit, no-separator format.

# Contracts
## C1 Pricing public contract (`pricing.mjs`)
```ts
type Item = { priceCents: number; quantity: number };
// Sums integer priceCents * integer quantity.
// Empty array => 0. Zero values are valid.
// Throws RangeError if any priceCents or quantity is negative or non-integer.
export function subtotal(items: Item[]): number;
```
Examples: `[] => 0`; `[{priceCents:125,quantity:2},{priceCents:50,quantity:3}] => 400`; `[{priceCents:0,quantity:9}] => 0`; `[{priceCents:-1,quantity:1}]` throws `RangeError`.

## C2 Format public contract (`format.mjs`)
```ts
// Cents => dollar string: exactly two decimals, no thousands separator.
// Negative sign precedes the dollar sign.
export function money(cents: number): string;
```
Examples: `0 => "$0.00"`, `1 => "$0.01"`, `105 => "$1.05"`, `-105 => "-$1.05"`, `120000 => "$1200.00"`.

## C3 Invoice public contract (`invoice.mjs`)
```ts
import { subtotal } from './pricing.mjs';
import { money } from './format.mjs';
// Returns "Total: " + money(subtotal(items)); propagates RangeError from subtotal.
export function invoice(items: Item[]): string;
```
Examples: `[] => "Total: $0.00"`; `[{priceCents:125,quantity:2},{priceCents:50,quantity:3}] => "Total: $4.00"`; `[{priceCents:-1,quantity:2}]` throws `RangeError`.

## C4 Repository conventions
- ES modules (`"type": "module"`); use named `export function`, no default exports.
- Tests are fixed in `check.mjs`; do not modify anything outside the three source files.
- Acceptance is run by Tokage via npm scripts; execution agents edit files only.

## C5 TaskReport
- After execution each agent returns a TaskReport: task id, files changed, summary of change, and notes/blockers.

# Tasks
```yaml
- id: P0.M1a
  domain: backend-api
  title: "Implement subtotal pricing logic"
  goal: "Make subtotal sum integer priceCents * quantity, return 0 for empty input, and throw RangeError for negative or non-integer prices or quantities."
  depends_on: []
  contracts: [C1, C4, C5]
  write_scope: ["pricing.mjs"]
  deliverables: ["subtotal implementation in pricing.mjs"]
  acceptance:
    commands: ["npm run test:pricing"]
    criteria:
      - "All pricing assertions in check.mjs pass (empty, normal sum, zero values, all RangeError cases)"
      - "Public export subtotal is preserved and no other file is modified"
  size: S
  difficulty: 1
- id: P0.M1b
  domain: backend-api
  title: "Implement money formatting"
  goal: "Make money(cents) return a dollar string with exactly two decimals, no thousands separator, and a leading minus before the dollar sign for negatives."
  depends_on: []
  contracts: [C2, C4, C5]
  write_scope: ["format.mjs"]
  deliverables: ["money implementation in format.mjs"]
  acceptance:
    commands: ["npm run test:format"]
    criteria:
      - "All formatting assertions in check.mjs pass (0, 1, 105, -105, 120000)"
      - "Public export money is preserved and no other file is modified"
  size: S
  difficulty: 1
- id: P0.M2
  domain: integration
  title: "Compose invoice over pricing and formatting"
  goal: "Make invoice(items) import subtotal and money and return 'Total: ' plus the formatted subtotal, propagating RangeError from pricing."
  depends_on: [P0.M1a, P0.M1b]
  contracts: [C1, C2, C3, C4, C5]
  write_scope: ["invoice.mjs"]
  deliverables: ["invoice implementation composing subtotal and money"]
  acceptance:
    commands: ["npm run test:integration", "npm test"]
    criteria:
      - "Integration assertions pass: 'Total: $0.00' for empty, 'Total: $4.00' for the mixed items, RangeError propagated for negative price"
      - "Full suite npm test passes for pricing, format, and integration"
  size: S
  difficulty: 1
```

# Notes
- Only `pricing.mjs`, `format.mjs`, `invoice.mjs` may change; tests, package files, config, and dependencies are frozen.
- Do not import third-party or Node formatting helpers that introduce grouping separators; compute the string directly.
- `P0.M1a` and `P0.M1b` are parallel with disjoint write scopes; `P0.M2` must not start until both land.
- Execution agents use file tools only; Tokage runs the shell acceptance commands independently, so agents must not run tests themselves.
- Environment is pre-provisioned with `npm ci`; no installs are needed.
- Return a TaskReport (per C5) after execution; no model fallback.
