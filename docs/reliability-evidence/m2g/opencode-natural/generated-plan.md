# Spec

## Goals
Implementation of three small Node ESM modules: `subtotal` validates and sums integer cent-based line items, `money` formats integer cents as dollar strings, and `invoice` composes both into a total line while propagating errors.

## Scope
- In scope: three exported functions in `pricing.mjs`, `format.mjs`, and `invoice.mjs`; pricing and formatting implemented independently in parallel; integration composed afterward.
- Out of scope: new helpers/modules, tests, package/config changes, dependencies, export renaming, UI or currency localization.

## Key decisions
- Decision: Keep the input shape `{ priceCents, quantity }` unchanged; all validation and summation happen inside `subtotal` so callers need no pre-validation.
- Decision: `money` operates on integer cents only, avoiding floating-point arithmetic entirely.
- Decision: `invoice` reuses the two public functions rather than reimplementing arithmetic or formatting.
- Decision: Pricing and formatting run as parallel tasks with disjoint write scopes; integration is serialized after both.

# Contracts

## C1 Pricing public API — `pricing.mjs`
```ts
/**
 * @param {Array<{ priceCents: number, quantity: number }>} items
 * @returns {number} integer sum of priceCents * quantity
 * @throws {RangeError} if any priceCents or quantity is not an integer, or is negative
 */
export function subtotal(items: Array<{ priceCents: number; quantity: number }>): number;
```
- Empty array returns `0`.
- Zero price and/or zero quantity are valid and contribute `0`.
- Validation applies per item; any invalid item causes a `RangeError`.
- Non-array or malformed items may throw; only finite integer validation is required.

## C2 Formatting public API — `format.mjs`
```ts
/**
 * @param {number} cents integer number of cents
 * @returns {string} e.g. "$0.00", "$1.05", "-$1.05", "$1200.00"
 */
export function money(cents: number): string;
```
- Always exactly two decimal digits; never a thousands separator.
- Negative values place the minus before `$` (e.g. `-105` → `-$1.05`).
- No rounding of fractional cents is required by the callers; format the integer input as given.

## C3 Integration public API — `invoice.mjs`
```ts
/**
 * @param {Array<{ priceCents: number, quantity: number }>} items
 * @returns {string} `"Total: " + money(subtotal(items))`
 * @throws {RangeError} propagated from subtotal for invalid items
 */
export function invoice(items: Array<{ priceCents: number; quantity: number }>): string;
```
- Must import `subtotal` from `./pricing.mjs` and `money` from `./format.mjs`.
- No validation logic duplicated in `invoice.mjs`; errors propagate unchanged.

## C4 Repository constraints
- Only `pricing.mjs`, `format.mjs`, `invoice.mjs` may change.
- Do not modify `check.mjs`, `package.json`, `package-lock.json`, or any test/config file; do not add dependencies.
- Preserve named exports exactly as declared above.
- Execution agents modify files with file tools only; shell acceptance is run by Tokage.

# Tasks

```yaml
- id: P0.M1a
  domain: algorithm
  title: "Implement integer-cent subtotal validation and summation"
  goal: "Implement subtotal(items) in pricing.mjs to return 0 for empty input and the integer sum of priceCents * quantity, throwing RangeError for any negative or non-integer priceCents or quantity while allowing zeros."
  depends_on: []
  contracts: [C1, C4]
  write_scope: ["pricing.mjs"]
  deliverables: ["subtotal implementation preserving the named export"]
  acceptance:
    commands: ["npm run test:pricing"]
    criteria:
      - "Empty input returns 0"
      - "125x2 plus 50x3 returns 400 and zero values remain valid"
      - "Negative or non-integer priceCents or quantity throws RangeError"
  size: S
  difficulty: 1
- id: P0.M1b
  domain: general
  title: "Implement money formatting for integer cents"
  goal: "Implement money(cents) in format.mjs to return a dollar string with exactly two decimal digits, no thousands separators, and the minus sign before the dollar sign for negative values."
  depends_on: []
  contracts: [C2, C4]
  write_scope: ["format.mjs"]
  deliverables: ["money implementation preserving the named export"]
  acceptance:
    commands: ["npm run test:format"]
    criteria:
      - "0, 1, 105, and 120000 format as $0.00, $0.01, $1.05, and $1200.00"
      - "Negative input formats with the minus before the dollar sign"
  size: S
  difficulty: 1
- id: P0.M2
  domain: integration
  title: "Compose invoice from pricing and formatting"
  goal: "Implement invoice(items) in invoice.mjs by importing subtotal from ./pricing.mjs and money from ./format.mjs, returning 'Total: ' plus the formatted subtotal and propagating RangeError from subtotal."
  depends_on: [P0.M1a, P0.M1b]
  contracts: [C1, C2, C3, C4]
  write_scope: ["invoice.mjs"]
  deliverables: ["invoice implementation preserving the named export and cross-module imports"]
  acceptance:
    commands: ["npm run test:integration", "npm test"]
    criteria:
      - "Empty input returns 'Total: $0.00'"
      - "125x2 plus 50x3 returns 'Total: $4.00'"
      - "Invalid negative item throws RangeError through invoice"
      - "Full combined suite passes"
  size: S
  difficulty: 1
```

# Notes
- P0.M1a and P0.M1b are parallel and must never write outside their own file; P0.M2 must wait for both and must not modify `pricing.mjs` or `format.mjs`.
- Use `node:` built-ins only (`node:assert` is test-side); no external packages.
- `invoice` must not recompute totals or reformat strings itself.
- Execution agents must use file tools only and finish with the required TaskReport format, including status, files changed, acceptance commands relied on, and notes. No model fallback.
- Tokage runs acceptance commands independently; agents should not assume shell results.