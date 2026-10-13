# Spec
## Goal
Exercise the complete Tokage pipeline with the Debug Provider.
## Scope
- In scope: scripted implementation, regression test, reports and delivery
- Out of scope: real model calls and changes to the original project
## Decisions
- All model output and acceptance feedback are simulated.
# Contracts
## C1 Debug result
```ts
export const debugResult: string;
```
# Tasks
```yaml
- id: T1
  domain: backend-api
  title: Produce the debug result
  goal: Export a deterministic debug result
  depends_on: []
  contracts: [C1]
  write_scope: ["src/debug-result.ts"]
  deliverables: ["debug result"]
  acceptance:
    commands: ["npm test"]
    criteria: ["Deterministic result is available"]
  size: S
  difficulty: 1
- id: T2
  domain: testing
  title: Verify the debug result
  goal: Verify the upstream result and report acceptance
  depends_on: [T1]
  contracts: [C1]
  write_scope: ["tests/debug-result.test.ts"]
  deliverables: ["regression test report"]
  acceptance:
    commands: ["npm test"]
    criteria: ["Upstream result matches the contract"]
  size: S
  difficulty: 1
```
# Notes
This is a simulated run. The original project is untouched.
