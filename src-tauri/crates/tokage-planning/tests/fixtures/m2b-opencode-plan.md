# Spec
## Goals
Implement three pure ES-module functions — `subtotal`, `money`, and `invoice` — so the existing `check.mjs` acceptance suite passes for pricing, formatting, and integration, without touching tests or configuration.

## Scope
- In scope: edit only `pricing.mjs`, `format.mjs`, `invoice.mjs`.
- Out of scope: editing `check.mjs`, `package.json`, `package-lock.json`, tests, config, or adding dependencies.

## Key decisions
- Decision: keep each function in its own single file; pricing and format are independent pure functions, so they run in parallel.
- Decision: integration is a thin composition (`invoice` imports `subtotal` + `money`) and must not reimplement logic.
- Decision: validation lives only in `subtotal`; `invoice` relies on error propagation rather than re-validating.
- Decision: integer arithmetic only; reject non-finite/non-integer/negative inputs with `RangeError`.

# Contracts
## C1 pricing.mjs — subtotal
```ts
// Public export, signature only
export function subtotal(items: Array<{ priceCents: number; quantity: number }>): number;
// Behavior contract:
// - return sum of priceCents * quantity over items
// - empty array => 0
// - priceCents or quantity negative OR non-integer => throw RangeError
// - zero is valid (priceCents:0 and/or quantity:0 contribute 0)
// - integer cents in, integer cents out
```

## C2 format.mjs — money
```ts
// Public export, signature only
export function money(cents: number): string;
// Behavior contract:
// - exactly two decimal digits, e.g. 0 => "$0.00", 1 => "$0.01", 105 => "$1.05"
// - no thousands separator: 120000 => "$1200.00"
// - negatives put the minus before the dollar sign: -105 => "-$1.05"
```

## C3 invoice.mjs — invoice (cross-module composition)
```ts
// Public export, signature only
import { subtotal } from './pricing.mjs'; // C1
import { money } from './format.mjs';     // C2
export function invoice(items: Array<{ priceCents: number; quantity: number }>): string;
// Behavior contract:
// - returns "Total: " + money(subtotal(items))
// - empty items => "Total: $0.00"
// - RangeError from subtotal propagates unchanged (no catch/re-wrap)
```

## C4 Module/export conventions
- ES modules (`.mjs`), named exports exactly as C1–C3; no default exports, no new exports.
- No imports beyond node built-ins and the sibling modules named in C3.

# Tasks
```yaml
- id: P0.M1a
  domain: algorithm
  title: Implement integer subtotal with validation
  goal: Implement subtotal(items) per C1 summing priceCents*quantity with empty->0 and RangeError on negative/non-integer values.
  depends_on: []
  contracts: [C1, C4]
  write_scope: ["pricing.mjs"]
  deliverables: ["pricing.mjs with named export subtotal"]
  acceptance:
    commands: ["npm run test:pricing"]
    criteria: ["TOKAGE_NATURAL_PRICING_OK printed", "empty=>0, 125*2+50*3=400, zero values valid, negative and non-integer throw RangeError"]
  size: S
  difficulty: 1

- id: P0.M1b
  domain: general
  title: Implement money formatting
  goal: Implement money(cents) per C2 producing two-digit dollar strings, no thousands separator, minus before dollar sign.
  depends_on: []
  contracts: [C2, C4]
  write_scope: ["format.mjs"]
  deliverables: ["format.mjs with named export money"]
  acceptance:
    commands: ["npm run test:format"]
    criteria: ["TOKAGE_NATURAL_FORMAT_OK printed", "0=>$0.00, 1=>$0.01, 105=>$1.05, -105=>-$1.05, 120000=>$1200.00"]
  size: S
  difficulty: 1

- id: P0.M2
  domain: integration
  title: Compose invoice from pricing and format
  goal: Implement invoice(items) per C3 importing subtotal (C1) and money (C2), returning "Total: " + formatted subtotal and propagating RangeError.
  depends_on: [P0.M1a, P0.M1b]
  contracts: [C1, C2, C3, C4]
  write_scope: ["invoice.mjs"]
  deliverables: ["invoice.mjs with named export invoice importing from ./pricing.mjs and ./format.mjs"]
  acceptance:
    commands: ["npm run test:integration", "npm test"]
    criteria: ["TOKAGE_NATURAL_INTEGRATION_OK printed", "empty=>'Total: $0.00', 125*2+50*3=>'Total: $4.00', negative price throws RangeError", "full gate npm test prints TOKAGE_NATURAL_ALL_OK"]
  size: S
  difficulty: 1
```

# Notes
- Execution agents use file tools only; Tokage runs the shell acceptance commands independently. `npm ci` is already performed before execution.
- Do not edit tests, `package.json`, `package-lock.json`, config, or add dependencies. Only `pricing.mjs`, `format.mjs`, `invoice.mjs` may change.
- Preserve existing public exports; add no new exports and no default exports.
- P0.M1a and P0.M1b are parallel with disjoint write scopes; P0.M2 must wait for both.
- After execution, each task must emit a TaskReport (task id, files changed, commands run by Tokage, acceptance evidence, blockers). No model fallback.
- `money` receives only non-negative integers from `subtotal`, but must still satisfy the negative-case contract in C2.