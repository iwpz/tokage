I'll read the current code and manifest first, since the digest doesn't show the existing exports or the test scripts.

The repo is small: three stub modules, a `check.mjs` harness that the npm scripts run, and no test files. `TaskReport` isn't in the repo. I'll check the hidden `.tokage/` directory and the memory folder for its definition, since ripgrep skips hidden paths by default.

# Spec
## Goals
Deliver a three-module invoice feature. `subtotal` (pricing.mjs) sums validated integer cents, `money` (format.mjs) renders dollar strings, and `invoice` (invoice.mjs) returns `"Total: "` plus the formatted subtotal. Pricing and formatting run in parallel; integration runs after both.

## Scope
- In scope: replace the three stub bodies (`subtotal` returns `0`; `money` and `invoice` return `"TODO"`) in `pricing.mjs`, `format.mjs`, and `invoice.mjs`; pass the npm gates listed in Tasks.
- Out of scope: any edit to `check.mjs`, tests, `package.json`, `package-lock.json`, `.gitignore`, or config; new dependencies; new exports; locale or currency formatting beyond `$`; overflow handling.

## Key decisions
- Invalid values throw RangeError: the spec names only RangeError; a TypeError for wrong types is the alternative.
- Empty input means `[]`: null, undefined, or non-array input is unspecified and not special-cased.
- No overflow guard: sums above `Number.MAX_SAFE_INTEGER` are unspecified; a safe-integer check on the total is the alternative.
- money has no input validation: invoice only passes subtotal's validated output; a RangeError check inside money is the alternative.
- `-0` formats as `$0.00`: only values below zero are negative.
- subtotal's RangeError propagates unchanged through invoice: the harness asserts the error type.
- No separate test tasks: `check.mjs` is the fixed domain harness, and Tokage runs its gates as acceptance.
- Domains: pricing and format are `general` (pure logic, no algorithmic complexity); invoice is `integration`.

# Contracts
## C1 Pricing: subtotal (pricing.mjs)
```ts
type PricingItem = { priceCents: number; quantity: number };
export function subtotal(items: PricingItem[]): number; // integer cents
```
- Returns the sum of `priceCents * quantity` over all items.
- `[]` returns `0`. A zero price or zero quantity is valid and contributes `0`.
- Throws `RangeError` if any `priceCents` or `quantity` is negative or fails `Number.isInteger` (fractions, NaN, ±Infinity, strings, undefined). All items are validated before any result is returned.

## C2 Formatting: money (format.mjs)
```ts
export function money(cents: number): string; // cents: integer
```
- Output is `$`, then whole dollars (no leading zeros; `0` for zero), then `.`, then exactly two zero-padded digits. No thousands separator.
- Negative input puts an ASCII hyphen-minus before `$`. Only values below zero are negative, so `-0` yields `$0.00`.
- Do not use `Intl` or `toLocaleString`.
- Examples: `0` → `$0.00`; `1` → `$0.01`; `105` → `$1.05`; `-105` → `-$1.05`; `120000` → `$1200.00`.
- No input validation.

## C3 Invoice: invoice (invoice.mjs)
```ts
export function invoice(items: PricingItem[]): string; // PricingItem: see C1
```
- Named-import `subtotal` from `./pricing.mjs` and `money` from `./format.mjs`. The `.mjs` extension is required by Node ESM. Do not reimplement summation or formatting here.
- Returns exactly `"Total: " + money(subtotal(items))`, with one space after the colon.
- A `RangeError` from `subtotal` propagates unchanged: no catch, no wrapping.
- Examples: `[]` → `"Total: $0.00"`; `[{priceCents:125,quantity:2},{priceCents:50,quantity:3}]` → `"Total: $4.00"`; `[{priceCents:-1,quantity:2}]` → throws `RangeError`.

## C4 Directory conventions
- Three modules at repo root: `pricing.mjs`, `format.mjs`, `invoice.mjs`. `package.json` sets `"type": "module"`.
- Fixed export names: `subtotal` (pricing.mjs), `money` (format.mjs), `invoice` (invoice.mjs). Add no exports; helpers stay module-private.
- Only these three files may change. `check.mjs` is the frozen acceptance harness, along with `package.json`, `package-lock.json`, `.gitignore`, and any config. Create no files and add no dependencies.
- Harness success lines: `TOKAGE_NATURAL_PRICING_OK`, `TOKAGE_NATURAL_FORMAT_OK`, `TOKAGE_NATURAL_INTEGRATION_OK`, `TOKAGE_NATURAL_ALL_OK`.

# Tasks
```yaml
- id: P0.M1a
  domain: general
  title: "Pricing: validated integer subtotal"
  goal: "Implement subtotal per C1 in pricing.mjs so the pricing gate passes"
  depends_on: []
  contracts: [C1, C4]
  write_scope: ["pricing.mjs"]
  deliverables: ["pricing.mjs exporting subtotal per C1"]
  acceptance:
    commands: ["npm run test:pricing"]
    criteria: ["Exit code 0 and output contains TOKAGE_NATURAL_PRICING_OK", "Only pricing.mjs changed", "subtotal is the only export"]
  size: S
  difficulty: 1
- id: P0.M1b
  domain: general
  title: "Format: money dollar strings"
  goal: "Implement money per C2 in format.mjs so the format gate passes"
  depends_on: []
  contracts: [C2, C4]
  write_scope: ["format.mjs"]
  deliverables: ["format.mjs exporting money per C2"]
  acceptance:
    commands: ["npm run test:format"]
    criteria: ["Exit code 0 and output contains TOKAGE_NATURAL_FORMAT_OK", "Only format.mjs changed", "money is the only export"]
  size: S
  difficulty: 1
- id: P0.M2a
  domain: integration
  title: "Invoice: compose subtotal and money"
  goal: "Implement invoice per C3 using the C1 and C2 exports, then pass the full gate"
  depends_on: [P0.M1a, P0.M1b]
  contracts: [C1, C2, C3, C4]
  write_scope: ["invoice.mjs"]
  deliverables: ["invoice.mjs exporting invoice per C3"]
  acceptance:
    commands: ["npm run test:integration", "npm test"]
    criteria: ["npm run test:integration exits 0 and prints TOKAGE_NATURAL_INTEGRATION_OK", "npm test exits 0 and prints TOKAGE_NATURAL_ALL_OK", "Only invoice.mjs changed"]
  size: M
  difficulty: 2
```

# Notes
- Order: P0.M1a and P0.M1b run in parallel, since their write scopes do not overlap. P0.M2a starts only after both are merged and their acceptance passes.
- Tooling: executors use file tools only and run no shell, npm, or node commands. The environment runs `npm ci` before execution. Tokage runs all acceptance commands independently, so reports must say verification was by reading.
- Model: no model fallback. Run only on the assigned model; if it is unavailable, fail the task and say so.
- Code: replace the stub bodies completely. Match the existing ESM named-export style, keep the code minimal, and add no logging or helper files.
- Open item (blocks only the report step): the "required TaskReport format" is defined neither in the request nor in this repository. Do not invent a schema; Tokage must supply the template. Until then, report the task id, changed files, and the verification performed. Please also confirm or change the Key decisions that list an alternative.