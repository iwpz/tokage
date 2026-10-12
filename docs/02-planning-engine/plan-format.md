# Plan Format and Planning Prompt Template

**English** | [中文](plan-format.zh-CN.md)

Back to [02 · Planning Engine](README.md)

## 1. Planning Output Format

The planning model’s output must be a Markdown document with exactly four top-level sections in fixed order. The task graph uses a YAML fenced block for local parsing. Milestone / module hierarchy lives only in task `id` (e.g. `P0.M1a`); **do not** add extra Markdown headings such as `# P0 milestone` or `# P0.M1 module`.

````markdown
# Spec
## Goals
One to three sentences.

## Scope
- In scope: …
- Out of scope: …

## Key decisions
- Decision: rationale (one line each)

# Contracts
## C1 User API
```ts
// Signatures and types only, no implementation
type User = { id: string; email: string; createdAt: string };
// GET /api/users/:id -> 200 User | 404
// POST /api/users  body: { email: string } -> 201 User | 409
```

## C2 Directory conventions
- Frontend API clients under web/src/api/, one file per resource

# Tasks
```yaml
- id: T1
  domain: backend-api
  title: User API
  goal: Implement user fetch and create per C1
  depends_on: []
  contracts: [C1]
  write_scope: ["src/server/users/**"]
  deliverables: ["GET/POST routes", "data access layer"]
  acceptance:
    commands: ["cargo test -p server users"]
    criteria: ["All status codes from C1 covered"]
  size: M
  difficulty: 2

- id: T2
  domain: frontend-ui
  title: User profile page
  goal: Profile page and create form per C1 User type
  depends_on: []
  contracts: [C1, C2]
  write_scope: ["web/src/pages/users/**"]
  deliverables: ["Profile page", "Create form (validation and error states)"]
  acceptance:
    commands: ["pnpm -C web typecheck"]
    criteria: ["Form keyboard-operable; errors associated with inputs"]
  size: M
  difficulty: 2

- id: T3
  domain: testing
  title: User module tests
  goal: API tests for each C1 status code; component tests for form
  depends_on: [T1, T2]
  contracts: [C1]
  write_scope: ["tests/users/**", "web/src/pages/users/**/*.test.tsx"]
  acceptance:
    commands: ["cargo test -p server", "pnpm -C web test"]
  size: S
  difficulty: 1
```

# Notes
Extra constraints for executors; may be empty.
````

Field reference:

| Field | Required | Description |
|------|------|------|
| `id` | yes | Unique in plan, `T` + number |
| `domain` | yes | From definition pack domain table ([04 knowledge-pack](../04-model-matching-engine/knowledge-pack.md)) |
| `depends_on` | yes | Upstream task ids; downstream receives upstream task reports |
| `contracts` | no | Referenced contract ids |
| `write_scope` | yes | Paths allowed for writes (relative to workspace root). Prefer globs (`src/server/**`, `**/*.ts`); directory `src/users/`, single file `src/main.rs`, `.` / empty string also allowed. Parse and `validate_plan` normalize via `command_policy::scope::normalize_scope` (strip `./`, `\`→`/`, collapse `/`, trim trailing `/`; append `/**` if no wildcard; `.` / empty → `**`). UI editor may keep raw input; validation normalizes on save. Parallel tasks must not overlap scopes (after normalization, `scope::overlap`; see [09 §4](../09-guardrails/README.md)); read-only tasks use empty array |
| `acceptance.commands` | no | Acceptance commands Tokage runs locally |
| `acceptance.criteria` | no | Criteria for executor self-check in task report |
| `size` | yes | `S` / `M` / `L`, for cost estimate |
| `difficulty` | yes | 1–3, for success probability estimate |

## 2. Planning Prompt Template

Fixed prefix first (identical every planning call, cacheable), variable suffix after.

```markdown
You are a technical planner. Your output will be split among multiple AI coding agents working in parallel.
They cannot see each other's context—only your contracts and their own task.

Requirements:
1. Write constraints only, not implementation code. Contracts use signatures, types, table schemas, status codes.
2. Tasks may use hierarchical ids (P0.M1a / P0.M1b …) for milestones and modules; hierarchy only in YAML id field. Do not output extra Markdown headings like "# P0 milestone" or "# P0.M1 module". Parallel tasks must not have overlapping write scopes.
3. Each task must be completable without asking other tasks; put needed information in contracts.
4. Each task must have locally runnable acceptance commands chosen from scripts in the "project digest".
5. Repetitive mechanical testing should be separate tasks with domain testing.
6. Total output length must not exceed {{output_max_tokens}} tokens. Use exactly four top-level headings: # Spec, # Contracts, # Tasks, # Notes; task list must be in a yaml fenced block under # Tasks.
7. Prefer the project digest; read files only when the digest is insufficient for interfaces.
8. domain must be chosen exactly from "Available domains" (aligned with knowledge pack: `frontend-ui`, `backend-api`, `data-model`, `algorithm`, `integration`, `refactor`, `infra`, `testing`, `docs`, `review`, `general`). Database/migrations → `data-model`; CI/deploy → `infra`; auth/security → `backend-api`. Do not use legacy tags `database` / `devops` / `security`.
9. size / difficulty anchors: small single-file edit S/1; routine multi-file feature M/2; cross-module or high-risk L/3.

Available domains: {{domains}}

Output format:
{{format_spec}}

---
# Project digest
{{repo_digest}}

---
# Request
{{user_request}}
```

For incremental replanning, append three sections before “Request”: “Current plan”, “Reports of affected completed tasks”, “Change description” (tradeoffs see [README §9](README.md)), and change requirement 6 to “Output diff only: added, modified, removed tasks and contracts”.
