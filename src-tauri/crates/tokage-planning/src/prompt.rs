//! Prompt template rendering for planning requests (docs/02 §5, docs/02 plan-format.md).

/// Default planning output format specification markdown injected into the prompt.
///
/// Outer fence uses four backticks so inner ```ts / ```yaml examples do not close early.
pub const PLAN_FORMAT_SPEC: &str = r##"````markdown
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
type User = { id: string; email: string };
```

## C2 Directory conventions
- Paths or conventions

# Tasks
```yaml
# Hierarchy lives only in id (P0.M1a / P0.M1b…). Do not add extra headings like "# P0 milestone" or "# P0.M1 module".
- id: P0.M1a
  domain: backend-api
  title: User auth and token issuance
  goal: Implement login verification and issue JWT tokens
  depends_on: []
  contracts: [C1]
  write_scope: ["src/auth/**"]
  deliverables: ["AuthService and token issuance"]
  acceptance:
    commands: ["cargo test -p tokage-auth"]
    criteria: ["All unit tests pass with no compile warnings"]
  size: M
  difficulty: 2
- id: P0.M1b
  domain: frontend-ui
  title: Login form and state sync
  goal: Wire the user API into the frontend login flow
  depends_on: [P0.M1a]
  contracts: [C1]
  write_scope: ["src/components/auth/**"]
  deliverables: ["LoginForm component"]
  acceptance:
    commands: ["npm test"]
    criteria: ["Form validation and API calls succeed"]
  size: S
  difficulty: 1
```

# Notes
Extra constraints for executors; may be empty.
````"##;

/// Default system instructions template for planning invocation (docs/02 plan-format.md §2).
pub const PLANNING_SYSTEM_TEMPLATE: &str = r##"You are a technical planner. Your output will be split among multiple AI coding agents working in parallel.
They cannot see each other's context—only your contracts and their own task.

Requirements:
1. Write constraints only, not implementation code. Contracts use signatures, types, table schemas, status codes.
2. Tasks may use hierarchical ids (P0.M1a / P0.M1b …) for milestones and modules; hierarchy only in YAML id field. Do not output extra Markdown headings like "# P0 milestone" or "# P0.M1 module". Parallel tasks must not have overlapping write scopes.
3. Each task must be completable without asking other tasks; put needed information in contracts.
4. Each task must have locally runnable acceptance commands chosen from scripts in the "project digest".
5. Repetitive mechanical testing should be separate tasks with domain testing.
6. Total output length must not exceed {{output_max_tokens}} tokens. Use exactly four top-level headings: # Spec, # Contracts, # Tasks, # Notes; task list must be in a yaml fenced block under # Tasks.
7. Prefer the project digest; read files only when the digest is insufficient for interfaces.
8. domain must be chosen exactly from "Available domains": database/migrations/ORM → data-model; CI/Docker/deploy → infra; auth and security logic → backend-api; complex algorithms/performance → algorithm; integration/conflicts → integration; large refactors → refactor. Do not use legacy tags like database, devops, or security. For multi-skill work, pick the domain of the main deliverable, or split into multiple tasks.
9. Quote YAML title and goal strings when they contain colons or quotes; use valid YAML escaping.
10. size / difficulty anchors: small single-file edit size=S, difficulty=1; routine multi-file feature size=M, difficulty=2; cross-module architecture or high-risk change size=L, difficulty=3.

Match the language of the user request for Spec / Contracts / Tasks / Notes prose (and your reasoning). Keep YAML keys, domain ids, and the four top-level headings in English as specified.

Available domains: {{domains}}

Output format:
{{format_spec}}

---
# Project digest
{{repo_digest}}

---
# Request
{{user_request}}"##;

/// Full knowledge-pack domain list (docs/04 §3), including role-only `planning`.
pub const PACK_DOMAINS: &[&str] = &[
    "planning",
    "frontend-ui",
    "backend-api",
    "data-model",
    "algorithm",
    "integration",
    "refactor",
    "infra",
    "testing",
    "docs",
    "review",
    "general",
];

/// Domains the planner may assign to execution tasks (excludes role-only `planning`).
pub const DEFAULT_DOMAINS: &[&str] = &[
    "frontend-ui",
    "backend-api",
    "data-model",
    "algorithm",
    "integration",
    "refactor",
    "infra",
    "testing",
    "docs",
    "review",
    "general",
];

/// Parameters required to construct a full planning prompt.
#[derive(Clone, Debug)]
pub struct PlanningPromptArgs<'a> {
    pub user_request: &'a str,
    pub repo_digest: &'a str,
    pub domains: Option<&'a [String]>,
    pub output_max_tokens: Option<usize>,
    pub custom_format_spec: Option<&'a str>,
}

/// Renders the complete planning prompt according to docs/02 plan-format.md.
pub fn render_planning_prompt(args: PlanningPromptArgs<'_>) -> String {
    let output_max_tokens = args.output_max_tokens.unwrap_or(6000);
    let format_spec = args.custom_format_spec.unwrap_or(PLAN_FORMAT_SPEC);

    let domains_str = if let Some(custom) = args.domains {
        custom.join(", ")
    } else {
        DEFAULT_DOMAINS.join(", ")
    };

    PLANNING_SYSTEM_TEMPLATE
        .replace("{{output_max_tokens}}", &output_max_tokens.to_string())
        .replace("{{domains}}", &domains_str)
        .replace("{{format_spec}}", format_spec)
        .replace("{{repo_digest}}", args.repo_digest.trim())
        .replace("{{user_request}}", args.user_request.trim())
}

/// Format-repair prompt based on [`PLAN_FORMAT_SPEC`] (F16.b — single source of truth).
pub fn render_plan_repair_prompt(raw_text: &str) -> String {
    format!(
        "Convert the following technical plan into Markdown that strictly follows the Tokage format.\n\
         It must contain these four top-level sections in this order, and the task graph must use a yaml fenced block:\n\n\
         {PLAN_FORMAT_SPEC}\n\n\
         Rules:\n\
         1. Do not describe tasks with Markdown tables; use a yaml fenced block.\n\
         2. Preserve all task splits, dependencies, goals, and file paths from the original plan.\n\
         3. Do not output headings like \"# P0 milestone\" or \"# P0.M1 module\"; express hierarchy only via task ids.\n\
         4. If Spec/Contracts are missing, invent the minimum usable content from tasks and referenced contract ids.\n\
         5. Output only the converted Markdown; no explanation.\n\n\
         Original content:\n\n{}",
        raw_text.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_planning_prompt() {
        let prompt = render_planning_prompt(PlanningPromptArgs {
            user_request: "Implement user registration",
            repo_digest: "## Directory structure\n- src/main.rs",
            domains: None,
            output_max_tokens: Some(4000),
            custom_format_spec: None,
        });

        assert!(prompt.contains("You are a technical planner"));
        assert!(prompt.contains("4000 tokens"));
        assert!(prompt.contains("Available domains: frontend-ui, backend-api, data-model"));
        assert!(prompt.contains("Do not use legacy tags like database, devops, or security"));
        assert!(!prompt.contains("database / devops"));
        assert!(prompt.contains("# Project digest\n## Directory structure\n- src/main.rs"));
        assert!(prompt.contains("# Request\nImplement user registration"));
        assert!(prompt.contains("# Spec"));
        assert!(prompt.contains("# Tasks"));
        assert!(prompt.contains("Use exactly four top-level headings"));
        assert!(prompt.contains("Do not output extra Markdown headings like \"# P0 milestone\""));
        assert!(prompt.contains("Match the language of the user request"));
        // Nested fences must not truncate the format example at ```ts / ```yaml.
        assert!(
            PLAN_FORMAT_SPEC.starts_with("````markdown"),
            "outer fence must be 4 backticks so inner code fences survive"
        );
        assert!(prompt.contains("write_scope:"));
        assert!(prompt.contains("# Notes"));
    }

    #[test]
    fn test_plan_format_spec_nested_fences_intact() {
        assert!(PLAN_FORMAT_SPEC.contains("```ts\n"));
        assert!(PLAN_FORMAT_SPEC.contains("```yaml\n"));
        assert!(PLAN_FORMAT_SPEC.contains("Do not add extra headings"));
        assert!(PLAN_FORMAT_SPEC.trim_end().ends_with("````"));
    }

    #[test]
    fn test_render_plan_repair_prompt_forbids_milestone_headers() {
        let prompt = render_plan_repair_prompt("# P0 milestone\n- id: P0.M1a\n  title: x\n");
        assert!(prompt.contains("# P0 milestone"));
        assert!(prompt.contains("Original content:"));
        assert!(prompt.contains("express hierarchy only via task ids"));
    }
}
