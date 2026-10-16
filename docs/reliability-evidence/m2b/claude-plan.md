I'll read the existing modules and the test/script wiring before planning, since the digest does not show the current interfaces.

# Spec
## Goals
Deliver a three-module invoice feature: an integer-cent subtotal with validation (pricing.mjs), dollar-string formatting (format.mjs), and an invoice total string built from both (invoice.mjs). Pricing and formatting run in parallel; integration follows and must pass the full npm test gate.

## Scope
- In scope: replace the stub bodies in pricing.mjs, format.mjs, and invoice.mjs (they currently return 0, "TODO", and "TODO") per contracts C1–C3.
- Out of scope: tests, check.mjs, package.json, package-lock.json, configuration, dependencies, new files, rounding, tax, discounts, locale formatting, thousands separators, precision beyond Number.MAX_SAFE_INTEGER.

## Key decisions
- Parallel split: P0.M1a and P0.M1b share no imports or write scope, and P0.M2a is the only consumer of both, so it waits for both.
- subtotal validation uses Number.isInteger plus a non-negative check: this matches check.mjs and makes NaN, Infinity, undefined, and numeric strings throw RangeError.
- money rejects non-integer cents with RangeError (assumption, not in the request): it mirrors subtotal and keeps the exact two-digit guarantee. Change this if rounding was intended.
- money uses integer arithmetic, not Intl or toLocaleString: locale formatting can add separators or non-ASCII minus signs.
- Non-array input is unspecified: the request does not define it, so a native TypeError is accepted. Returning 0 or throwing RangeError were rejected as guesses.
- invoice does not catch errors: the request says to propagate, and check.mjs asserts RangeError from invoice.
- No testing-domain task: check.mjs is fixed and already defines the checks, so npm test is attached to P0.M2a as requested.
- Domains: pricing is algorithm, format is general, invoice is integration. All three are single-file edits (size S, difficulty 1).

# Contracts
## C1 Pricing: subtotal
```ts
type LineItem = { priceCents: number; quantity: number };
export function subtotal(items: LineItem[]): number; // pricing.mjs
```
- Returns integer cents: the sum over items of priceCents × quantity.
- `[]` returns 0. Zero priceCents or quantity is valid. A zero total must be plain 0, not -0 (check.mjs uses strict equality, which distinguishes -0).
- Each priceCents and quantity must satisfy Number.isInteger and be ≥ 0; otherwise throw RangeError. This includes negatives, NaN, Infinity, undefined, and numeric strings.
- Validate before returning, so no partial sum is ever returned. Extra item properties are ignored.
- Non-array items and null entries are unspecified; a native TypeError is acceptable. Add no checks for them.

## C2 Format: money
```ts
export function money(cents: number): string; // format.mjs
```
- Input is integer cents; negative values are allowed. Non-integers (including NaN, Infinity, numeric strings) throw RangeError. No rounding.
- Output: "$" + whole dollars with no separators + "." + exactly two zero-padded cent digits. Negatives get "-" before "$".
- Negative zero formats as "$0.00".
- Must hold: 0 → "$0.00"; 1 → "$0.01"; 105 → "$1.05"; -105 → "-$1.05"; 120000 → "$1200.00".
- Do not use Intl or toLocaleString.

## C3 Invoice: invoice
```ts
export function invoice(items: LineItem[]): string; // invoice.mjs, LineItem as in C1
```
- The only imports are named `subtotal` from './pricing.mjs' and named `money` from './format.mjs'.
- Returns the text "Total: " (one space after the colon) followed by money applied to subtotal(items).
- Must hold: `[]` → "Total: $0.00"; `[{priceCents:125,quantity:2},{priceCents:50,quantity:3}]` → "Total: $4.00".
- Errors from subtotal propagate unchanged. No try/catch and no re-wrapping.

## C4 Directory and execution conventions
- All three files live at the repo root. package.json sets "type": "module" and must not change.
- Relative imports use './' and the '.mjs' extension.
- Plain JavaScript only (no TypeScript annotations). Use named `export function` and keep the existing names: subtotal, money, invoice. No default exports and no additional exports. Private (non-exported) helpers are allowed.
- No top-level side effects or console output. No imports from check.mjs or node:assert.
- No new files.

# Tasks
```yaml
- id: P0.M1a
  domain: algorithm
  title: "Pricing: integer subtotal with validation"
  goal: "Implement subtotal(items) in pricing.mjs per C1: sum of priceCents times quantity, 0 for empty input, RangeError on invalid values"
  depends_on: []
  contracts: [C1, C4]
  write_scope: ["pricing.mjs"]
  deliverables: ["subtotal(items) in pricing.mjs"]
  acceptance:
    commands: ["npm run test:pricing"]
    criteria: ["Command exits 0 and prints TOKAGE_NATURAL_PRICING_OK", "Zero values are accepted; negative, non-integer, NaN, and Infinity values throw RangeError", "Only subtotal is exported, with the signature in C1"]
  size: S
  difficulty: 1
- id: P0.M1b
  domain: general
  title: "Format: dollar string from cents"
  goal: "Implement money(cents) in format.mjs per C2: two cent digits, no thousands separator, leading minus before the dollar sign for negatives"
  depends_on: []
  contracts: [C2, C4]
  write_scope: ["format.mjs"]
  deliverables: ["money(cents) in format.mjs"]
  acceptance:
    commands: ["npm run test:format"]
    criteria: ["Command exits 0 and prints TOKAGE_NATURAL_FORMAT_OK", "money(-105) returns '-$1.05' and money(120000) returns '$1200.00'", "Negative zero returns '$0.00' and non-integer input throws RangeError", "Only money is exported, with the signature in C2"]
  size: S
  difficulty: 1
- id: P0.M2a
  domain: integration
  title: "Invoice: compose subtotal and money"
  goal: "Implement invoice(items) in invoice.mjs per C3: import subtotal and money, return 'Total: ' plus the formatted subtotal, and propagate errors"
  depends_on: [P0.M1a, P0.M1b]
  contracts: [C1, C2, C3, C4]
  write_scope: ["invoice.mjs"]
  deliverables: ["invoice(items) in invoice.mjs"]
  acceptance:
    commands: ["npm run test:integration", "npm test"]
    criteria: ["npm run test:integration exits 0 and prints TOKAGE_NATURAL_INTEGRATION_OK", "npm test exits 0 and prints TOKAGE_NATURAL_ALL_OK (final full gate)", "invoice.mjs imports only subtotal from ./pricing.mjs and money from ./format.mjs", "No try/catch, and no re-implemented sum or formatting in invoice.mjs", "Only invoice is exported, with the signature in C3"]
  size: S
  difficulty: 1
```

# Notes
- Executors use file read, write, and search tools only. Do not run npm, node, or any shell command; Tokage runs every acceptance command after the task.
- Read check.mjs for the expected values, but do not edit it. It is the acceptance oracle.
- Edit only your write_scope file. Do not edit tests, package.json, package-lock.json, .gitignore, or configuration; do not add dependencies or files.
- Keep the diff minimal: replace the stub body and add only the imports the contract requires.
- No model fallback: if your assigned model fails or is unavailable, stop and report it; do not switch models.
- Open item: the request requires a TaskReport after execution but does not define its format. Confirm the schema before execution. If none is supplied, end with: task id, status, changed files, and each acceptance criterion marked pass or fail with evidence from reading the code.
