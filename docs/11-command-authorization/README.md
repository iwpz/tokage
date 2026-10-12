# 11 · Command Authorization & Credential Isolation

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

> Current state (2026-10-05): P1 and P2 phases complete and accepted. P2.m1 command lists and matcher, P2.m2 auth modes and security settings, P2.m3 approval channel and denial-resume, P2.m4 macOS Seatbelt sandbox, P2.m5 credential isolation (shell wrap, output redaction, diff alert), P2.m6 full-auto constraints and snapshot, P2.m7 Linux Landlock and Bubblewrap sandbox—all landed; unit and FakeAdapter scenario tests (R13–R17) green. See [TODO](../TODO.md).

## 1. Responsibilities

1. Commands and file operations initiated by the model inside the CLI are allowed, prompted, or denied per the user’s authorization mode.
2. Maintain tiered command lists shipped with definition packs; user may override per project.
3. Use OS-level sandbox to limit each CLI process tree to allowed files and network.
4. Block model reads of API keys, Base URL, and other credentials; ensure Tokage itself reads only non-secret fields.
5. Record each authorization decision for audit and “always allow” reuse.

Commands run inside the CLI; Tokage is not on their execution path. This chapter’s measures are layered by strength—earlier layers are more reliable:

| Layer | Measure | Nature |
|-------|---------|--------|
| L1 OS sandbox | Wrap entire CLI process tree (§5) | Security boundary, cannot bypass |
| L2 Approval callback | CLI calls Tokage before execute; user decides (§4.2) | Precise, depends on CLI support |
| L3 Pre-auth lists | Pass allow / deny rules at launch (§3) | UX layer, prefix match, bypassable variants—not a security boundary |
| L4 Post-hoc | git snapshot, scope check, one-click revert ([09 §4](../09-guardrails/README.md)) | Remediation only |

## 2. Authorization modes

| Mode | Auto-allow | Prompt | Deny |
|------|------------|--------|------|
| `ask_always` Ask every time | Read-only | Everything else (incl. file writes) | Red lines |
| `ask_sensitive` Ask sensitive (default) | Read-only, routine project, file edits within `write_scope` | Sensitive | Red lines |
| `full_auto` Full auto | All except red lines | None | Red lines |

- Modes apply to `EXECUTOR` / `TESTER` only. `PLANNER` / `REVIEWER` are read-only in all modes (only resolved `read_only` allowed, rest denied); `TESTER` denies `sensitive`, `project` level (incl. test commands) per mode.
- Unresolved (`resolved: false`) commands always prompt in every mode, including `full_auto`.
- File edits (CLI built-in edit tools, not shell) via `Policy::classify_edit`: within workspace and `write_scope` → `project`; hits `write:` rules (e.g. `.env*`), writes to `.git`, or outside `write_scope` → `sensitive`; outside workspace or credential paths → `forbidden`.
- `write_scope` matching uses `command_policy::scope` (glob relative to workspace root): `normalize_scope` strips `./`, maps `\` to `/`, collapses `//`, trims trailing `/`; `.` and empty string become `**` (whole workspace); entries without `*` / `?` / `[` become `<entry>/**` (so `src/users/` and `src/main.rs` cover paths beneath). `covers` / `overlap` normalize then call `path::{glob,overlap}`; `fold_case` follows `Context.case_insensitive` (macOS / Windows default true). Planning validation, scheduler collision, gate scope check, and `classify_edit` share this semantics.
- Scope: global default → project override → session temporary override; narrow beats wide (`auth::Layers::resolve`). Global and project may only store `ask_always` / `ask_sensitive`; session override is in-memory only, lost on restart. If `full_auto` unavailable for a CLI, skip that layer and take the next mode.

Extra constraints for `full_auto`:

- Second confirm on enable, listing risks: model may delete, rewrite, install deps over network inside workspace; Tokage only guarantees red lines and sandbox.
- Current project only; after app restart revert to `ask_sensitive`, optional TTL 1 hour / today / this session.
- Must run in worktree; git snapshot (incl. untracked list) before each turn.
- Only for CLIs with L1 sandbox; others grayed in picker with reason. Windows v1 not offered.
- Persistent warning bar with remaining TTL and “Exit full auto now”.
- Implemented as “broad allow list + red-line deny + sandbox”, not CLI skip-all-permissions flags (R8).

## 3. Command lists

### 3.1 Tiers

| Level | Examples | Source |
|-------|----------|--------|
| `read_only` | `ls` `pwd` `cat` `head` `tail` `wc` `rg` `grep` `git status/diff/log/show/branch` | Definition pack |
| `project` | Build / test / lint from repo digest, e.g. `npm test` `cargo build` | RepoDigest ([02 §3](../02-planning-engine/README.md)) + user confirm |
| `sensitive` | `rm` `mv` `cp -r` `chmod` `git reset/checkout/clean/push/rebase` `npm/pip/cargo install` `docker` `curl` `wget` `ssh`, read/write in-project `.env*` | Definition pack |
| `forbidden` | `sudo` `su`, writes outside workspace, `curl … \| sh`, `git push --force` to default branch, read credential paths (§6.1), `security` (keychain), process env reads other than `env` / `printenv` (`ps e`, `/proc/*/environ`) | Definition pack; user may only append |

### 3.2 Matching

When Tokage approves itself (L2, “denial resume”):

- Parse as shell into command segments; every segment in `&&` `||` `;` `|` `$()` backticks must be allowed for the whole to be allowed.
- Parse failure or inline interpreters like `eval` / `bash -c` / `python -c` → always `sensitive`.
- Path sensitivity: for `rm` etc., parse targets; paths inside `write_scope` and git-tracked treated as recoverable (`ask_sensitive` still prompts but notes “revertible”); target outside workspace → `forbidden`. Whether relative paths are in `write_scope` uses `scope::covers` like `classify_edit`, not bare string prefix.

L3 rules passed to CLI are an approximate projection of the list; not guaranteed to match Tokage parsing.

### 3.3 Pack fields

```json
"command_policy": {
  "read_only": ["ls", "pwd", "cat", "git status", "..."],
  "sensitive": ["rm", "mv", "git push", "npm install", "..."],
  "forbidden": ["sudo", "su", "security", "..."],
  "forbidden_paths": ["~/.ssh/**", "~/.aws/**", "..."]
}
```

Local overrides in `command_rules` ([08 schema](../08-storage/schema.md)); `forbidden` may only be appended, not removed.

Pattern syntax (same in pack and `command_rules`):

| Pattern | Meaning |
|---------|---------|
| `git push` | Command name + prefix positional args; rest unrestricted |
| `git push --force <default-branch>` | Must include flag; `<default-branch>` matches push target as default branch (incl. `+ref`, `HEAD:main`, current branch when no refspec) |
| `node --version` | Allow-level flagged patterns match exactly those flags; extra args → no match |
| `curl * \| sh` | Left command before right in same pipeline |
| `read:<glob>` / `write:<glob>` | Path rules; globs without `/` match filenames; relative paths relative to workspace root |
| `write:<outside-workspace>` | Write outside workspace |

Decision rules:

- Red line hit → `forbidden`; else user rules win (more specific wins), then highest level in pack; unknown → `sensitive`.
- Pack `read_only` applies only in built-in pack; custom pack `read_only` ignored; `sensitive` / `forbidden` / `forbidden_paths` append.
- When adding rules: rules below `forbidden` must not cover any red-line command, pipeline pattern, or credential path.
- Strip wrappers before match: `env` `command` `exec` `nohup` `nice` `timeout` `time` `xargs` `stdbuf`, etc.; `find -exec`, `fd -x`, `watch`, `trap`, `eval`, `bash -c`, here-doc commands judged per segment.
- Quotes, escapes, `$'…'`, brace expansion restored first; when variables / command substitution have no static value, command name matched against red lines as wildcards, rest marked unresolved (`resolved: false`); auth modes always prompt unresolved commands.
- Remote scripts: network fetch commands (`curl` `wget` etc.) whose output enters an interpreter via pipe, `$(…)`, `<(…)` → `forbidden`.
- Paths lexically normalized (`..`, `//`, `$HOME`, `~`), symlinks not resolved (sandbox backs up); read credential path → `forbidden`; recursive command rooted in credential dir → `sensitive`; write targets (redirect, `rm` `cp` `mv` `tee` `sed -i` `curl -o` `tar -x`, etc.) outside workspace → `forbidden`; relative paths after `cd` resolved from new cwd.
- Result includes reason list, write paths, and “revertible” judgment (all targets git-tracked).

## 4. Per-CLI implementation

### 4.1 Capability flags

`AdapterMeta.caps` adds:

| Cap | Meaning |
|-----|---------|
| `APPROVAL_CALLBACK` | Pre-execute external approval callback |
| `TOOL_RULES` | Launch-time allow / deny rules |
| `DENIAL_EVENT` | Structured “tool denied” in output |
| `SHELL_WRAP` | May replace / wrap shell or env for model commands |
| `OS_SANDBOX` | May be wrapped by Tokage OS sandbox and work correctly |
| `OWN_SANDBOX` | CLI has built-in sandbox |

### 4.2 Current state and plan

| CLI | Current | L2 | L3 | To verify |
|-----|---------|----|----|-----------|
| Claude Code | Plan: `--permission-mode plan`; Direct: `acceptEdits` + `READONLY_SHELL` | `--permission-prompt-tool`, Tokage built-in MCP approval tool | `--allowedTools` / `--disallowedTools` | MCP approval under `-p` behavior and timeout |
| Codex CLI | Plan / Direct both `-c sandbox_mode=read-only` (P1.m1c temporary downgrade), `exec` no prompt | `exec` none; `app-server` protocol has approval events | No command-level rules | `app-server` stability; nesting with outer sandbox |
| Cursor CLI | Plan: `--mode plan`; Direct no permission args, relies on CLI default allowlist (see measurements below); Tokage rejects Direct if config unsafe | TBD | Rules in `.cursor/cli.json`, conflicts with “don’t modify config” | Headless default (shell executed directly?) |
| Copilot CLI | Plan: `--plan` + deny write/shell; Direct: allow write + read-only shell | TBD (ACP) | `--allow-tool` / `--deny-tool` | Denied event format |

Measurement log (2026-10-04, P1.m1d):

| CLI | Version | Conditions | Result |
|-----|---------|------------|--------|
| Cursor CLI | 2026.10.01 | `-p --output-format stream-json --trust`, default config (`approvalMode: "allowlist"`, `permissions.allow: ["Shell(ls)"]`; fresh HOME same defaults) | `rm` not run, no prompt; `shellToolCall.result` `{"rejected": {"command", "reason", "isReadonly"}}`, usable as `DENIAL_EVENT` |
| Cursor CLI | Same | Also `--force` with `--mode ask` / `--mode plan` | Model did not call shell, files kept |
| Codex CLI | 0.160.0 | `exec -s workspace-write -c approval_policy="on-request"` | `rm` ran directly, no approval event |
| Codex CLI | Same | `approval_policy="untrusted"` | Unsupported, CLI exits with error |
| Codex CLI | Same | `exec -s read-only` | Edit and `rm` sandbox-denied (Operation not permitted) |

Conclusion: Cursor headless safety depends on `~/.cursor/cli-config.json` and project `.cursor/cli.json` `approvalMode` and `permissions.allow`. Tokage read-only probes these before each turn: if `approvalMode` is not `allowlist`, or allowlist has non-read-only `Shell(...)` (e.g. `Shell(*)`, `Shell(rm)`), Cursor excluded from Direct (picker + CLI panel explain). Tokage does not pass `--force` / `--yolo`.

Interim requirements before all caps confirmed:

- Codex Direct: per P2.m3d, Codex `app-server` is still experimental (`[experimental]`) and designed as a global shared daemon—conflicts with Tokage per-turn isolated process groups and outer OS sandbox (P2.m4); steady state—start with `exec -s read-only`, sandbox denial events feed denial-resume; after user approval switch to `-s workspace-write` and `resume` same turn.
- Cursor Direct: usable under default allowlist; Direct refused when config would run commands without prompt (P1.m1e). Denial-resume in P2.m3e.

**Interactive terminal resume (F10 / A.15)**: Kanban “Terminal takeover” via IPC `terminal_resume_command` produces one paste line (`cd '<cwd>' && …`). argv from each Adapter `terminal_resume` (aligned with headless `launch`; Codex interactive is `codex resume <id>`, not `exec`):

| Provider | Command | Resume | Notes |
|----------|---------|--------|-------|
| `CLAUDE_CODE` | `claude` | `claude --resume <id>` | |
| `CODEX_CLI` | `codex` | `codex resume <id>` | |
| `CURSOR_CLI` | `cursor-agent` | `cursor-agent --resume <id>` | |
| `COPILOT_CLI` | `copilot` | `copilot --resume <id>` | |
| `ANTIGRAVITY_CLI` | `agy` | `agy --resume <id>` | |
| `OPENCODE` | `opencode` | `opencode --session <id>` | |
| `AIDER` | `aider` | None (`null`) | UI suggests `aider --restore-chat-history` in task dir |

Without CLI `threadId`, also `null`; UI shows “No session to resume”.

### 4.3 Denial resume (generic fallback)

For CLIs without `APPROVAL_CALLBACK` but with `TOOL_RULES` + `DENIAL_EVENT`:

```
Start turn with current list → CLI denies command and emits denial event
  → Tokage pauses task, shows command in approval queue
  → User approves: add to session list, resume turn (“Approved, please continue”)
  → User denies: resume telling model command denied and to change approach
```

List change changes launch args; resume may lose cache prefix—approval UI warns cost.

**Pipeline tasks (F6)**: Sub-agent `approvalRequest` stored in task `taskApprovals`; kanban card and inspector reuse `ApprovalCard` (not chat approval queue). If pending approvals when turn ends (`done`, or `chat-ended` with neither `done` nor `error`) → task `BLOCKED` (“Waiting for approval”), **no gate**, release concurrency slot. After user decision, `resolveApproval` IPC updates `taskAllowed`, new turn with `resume = threadIds[taskId]`, `sessionAllowed = taskAllowed[taskId]`, `role = EXECUTOR` (prompt “User approved / denied command …”); without thread id cannot resume, UI “Re-run”. If decision while CLI turn still running, record only, resume after `chat-ended`.

## 5. OS-level sandbox

### 5.1 Platforms

| Platform | Implementation | Files | Network |
|----------|----------------|-------|---------|
| macOS | `sandbox-exec` + one SBPL profile per CLI | Path read / write | Only to Tokage local proxy port ([12](../12-network-proxy/README.md)) |
| Linux | Landlock (applied in child before spawn), bubblewrap fallback on old kernels | Path read / write | Network namespace or Landlock port rules |
| Windows | Not in v1 | — | — |

Sandbox wraps entire CLI; read/write tools and model-spawned children inside CLI are constrained equally.

### 5.2 Sandbox profile

One per CLI, shipped in pack (`sandbox_profiles`):

| Category | Content |
|----------|---------|
| Writable | Current worktree, system temp, that CLI’s session / cache dirs, package manager caches |
| Readable | Project, system libs and toolchain, that CLI’s config and credentials |
| Deny | Other CLIs’ dirs, §6.1 common credential paths, Tokage data dir |
| Keychain | Only CLIs that need it (e.g. Claude Code on macOS) |

Codex built-in sandbox limits writes only, not reads—still needs outer profile. If macOS cannot nest sandboxes, Codex relies on §6 env trim and shell wrap only; remaining risk flagged in CLI panel; do not disable Codex sandbox to get outer sandbox.

**P2.m4 implementation notes (macOS Seatbelt / SBPL)**:
- **SBPL generation (`tokage_native::sandbox`)**:
  - Base policy `(version 1) (allow default)`, writes strictly limited `(deny file-write*)`.
  - Write allowlist: `/dev/null`, `/dev/zero`, `/dev/tty`, `/dev/fd`, `{worktree}`, `{tmp}` (`/tmp`, `/private/tmp`, system temp), each CLI cache dir.
  - Credential red lines: `{forbidden_paths}`, `{other_cli_dirs}`, `{tokage_app_data}/**` as `(deny file-read* file-write* ...)`. Dirs CLI declares in `read` or `write` (e.g. Claude `~/.claude/**`) excluded from deny so CLI loads normally.
  - Keychain (P2.m4c): default deny keychain read (`~/Library/Keychains`, `/Library/Keychains`) and `(deny mach-lookup (global-name "com.apple.SecurityServer"))`; allow only when profile declares `keychain: true` (e.g. Claude OAuth).
  - Process wrap (`chat::run_cli`): CLIs with `Caps::OS_SANDBOX` (Claude Code, Cursor CLI, Copilot CLI) on macOS launched via `/usr/bin/sandbox-exec -p <sbpl>`.
- **Codex nested sandbox (P2.m4e)**:
  - Running Codex read inside macOS Seatbelt, Codex internal tool subprocess sandbox fails: `The command couldn't run because the sandbox failed: Operation not permitted.`
  - Apple Seatbelt forbids nested second Seatbelt without special kernel entitlement.
  - Design: Codex keeps `Caps::OWN_SANDBOX`, **not** wrapped in outer `sandbox-exec`; write limited by Codex sandbox; read/env risk via §6 (R16) and shell wrap.
- **P2.m7 (Linux Landlock and Bubblewrap fallback)**:
  - **Landlock (`tokage_native::sandbox::landlock`, P2.m7a)**:
    - Linux 5.13+ non-privileged Landlock in child `pre_exec`.
    - Ruleset claims full FS read, write, execute, truncate (`handled_access_fs`).
    - Allow read/execute system dirs (`/bin`, `/sbin`, `/usr`, `/lib`, `/lib64`, `/etc`, `/proc`, `/sys`, `/dev`) and project root.
    - Allow read/write worktree, temp (`/tmp`, `/var/tmp`), standard devices, CLI caches (`~/.claude`, `~/.npm`).
    - Default-deny: rest of `$HOME` including `~/.ssh`, `~/.aws`, `~/.codex`, all `forbidden_paths`, Tokage data → `EACCES`.
  - **Bubblewrap fallback (`tokage_native::sandbox::bwrap`, P2.m7b)**:
    - When Landlock unavailable, auto-detect and use `bwrap`.
    - RO bind root (`--ro-bind / /`), RW bind worktree and temp, credential dirs masked with `--tmpfs` and `--ro-bind-try /dev/null` on files.
    - CLI config paths (e.g. `~/.claude`) RW bind preserved.
  - **Cross-platform wrap (`tokage_native::sandbox::create_sandboxed_command`)**:
    - macOS `sandbox-exec -p <sbpl>`, Linux `bwrap` or Landlock, safe degrade without sandbox.
    - FakeAdapter tests: credential path reads blocked; full-auto out-of-workspace `rm` blocked on Linux and macOS.


## 6. Credential isolation

### 6.1 Credential paths

Shipped in pack (`forbidden_paths`); user may only append:

| Category | Paths |
|----------|-------|
| AI CLI | `~/.claude/` `~/.claude.json` `~/.codex/` `~/.cursor/` `~/.copilot/` `~/.config/github-copilot/` `~/.config/opencode/` `~/.local/share/opencode/` `~/.aider*` and other allowlisted CLI config dirs |
| General | `~/.ssh/` `~/.aws/` `~/.config/gcloud/` `~/.azure/` `~/.kube/` `~/.docker/config.json` `~/.npmrc` `~/.netrc` `~/.git-credentials` `~/.config/gh/` |
| System | Keychain, browser profile dirs |
| Tokage | `app_data_dir()` |

In-project `.env*` not red lines; treated as `sensitive`.

### 6.2 Model paths to credentials and countermeasures

CLI process must read its own credentials; model tools run inside that CLI, so “same CLI credentials” cannot be solved by process sandbox alone. Countermeasure: model commands run with less privilege than the CLI:

| Path | Example | Countermeasure | Strength |
|------|---------|----------------|----------|
| Read other CLI credentials | Claude reads `~/.codex/auth.json` | L1 sandbox deny | Strong |
| Read other vendor env vars | `echo $OPENAI_API_KEY` in Claude process | Child env trimmed per vendor (§6.3) | Strong |
| Shell read this CLI config | `cat ~/.claude/settings.json` | Shell wrap: model commands in stricter sub-sandbox, deny credential paths | Strong (needs `SHELL_WRAP`) |
| Shell read this CLI env | `env`, `echo $ANTHROPIC_API_KEY` | Shell wrap: strip `*_API_KEY` `*_TOKEN` `*_SECRET` `*_BASE_URL` before run | Strong (needs `SHELL_WRAP`) |
| CLI built-in read file | Claude Read on `~/.claude/**` | L3 deny rules | Medium |
| Keychain | `security find-generic-password` | Sandbox deny + `forbidden` command | Stronger |
| Read other process env | `ps eww`, `/proc/<pid>/environ` | Linux sandbox deny; macOS: BSD `ps` suppresses cross-process env by default, no `/proc`, `task_for_pid` limited by SIP and Seatbelt—normal same-user process cannot read others | Strong |

`SHELL_WRAP` implementation (P2.m5a, Claude Code, Codex CLI, Copilot CLI have `Caps::SHELL_WRAP`):
- **Claude Code**: Bash follows `$SHELL`; Tokage generates wrapper `/tmp/tokage/tokage-shell-wrapper.sh` (`unset` known vendor creds and `*_API_KEY`, `*_TOKEN`, `*_SECRET`, etc.), sets `SHELL` to wrapper.
- **Codex CLI**: Native flag `-c shell_environment_policy.inherit="core"` strips non-core vars.
- **Copilot CLI**: Native `--secret-env-vars=<CREDENTIAL_ENV>`, Copilot strips and redacts in output from shell and MCP children.

### 6.3 Child environment assembly

CLI dialog turn:

```
Login shell environment
  → Remove all known vendor credential vars (pack `credential_env` + wildcards *_API_KEY / *_TOKEN / *_SECRET)
  → Add back vars this CLI needs (pack `providers.<id>.env_allow`)
  → Proxy vars per [12](../12-network-proxy/README.md)
  → Remove host session vars (existing `env_remove`, e.g. CLAUDE_CODE_*)
```

Acceptance gate (`assemble_gate_env`, F5.d): from login env, delete vars matching `is_credential`, delete vars whose values contain URL userinfo (`://user:pass@`); keep names in `gateEnvAllow` (exact or `PREFIX_*`). Gate does not re-add vendor keys per Provider; default empty allowlist.

### 6.4 Output fallback

- Redact before persist and display (`tokage-native::chat::redact::Redactor`):
  - At turn start extract all known credential values from `env.vars` (length ≥ 6), replace longest-first with `[REDACTED]`.
  - Load pack `builtin.json` 10 regex classes (`redaction_patterns`, `sk-ant-`, `sk-`, `ghp_`, `github_pat_`, `AKIA`, `AIza`, etc.) compile and replace.
  - All streaming text, thinking blocks, tool details, tool output, file change diffs, approval requests, errors redacted before `Bus`, frontend, or SQLite.
- End-of-turn diff scan: sensitive key in new `FileChange` content → `EventKind::Warning` and top UI highlight.
- Redaction ensures keys do not enter Tokage UI/DB; cannot undo what model already saw—stated in settings.

## 7. Tokage read boundaries (R11)

Tokage process (not model) reads CLI config for display and billing:

| Allowed read | Purpose | Handling |
|--------------|---------|----------|
| Base URL | CLI panel | Strip userinfo, query, fragment; path segments that look like tokens → `***` (`usage::display_url`) |
| Auth method, plan, account type | Billing type ([06 §2](../06-usage-melting-engine/README.md)) | Declared fields only |
| Key var names, credential file existence | Login state | No values |

| Forbidden | |
|-----------|-|
| Read Key / Token values | Discard on JSON parse, never in struct fields |
| Persist, log, send to frontend | Incl. debug and crash reports |
| Auth / quota requests to model vendors | Tokage is neutral integrator; usage queried by local CLI ([06 §4](../06-usage-melting-engine/README.md)) |

Allowed outbound (non-model infra): pack download, official pack update check (after signature verify; channel off until enabled, [04 §10](../04-model-matching-engine/README.md)).

Token-like path segments: common key prefixes (`sk-` `ghp_` `github_pat_` `AIza` `AKIA`, etc.), or length ≥ 32, or length ≥ 16 with 3+ letters and digits. Readable segments like `v1`, `openai`, `chat-completions` kept.

Test `usage::tests::secrets_are_dropped_on_read` puts keys in env, `settings.json`, `auth.json`, `config.toml`, Base URL userinfo / path / query; asserts `Account` IPC JSON and `Debug` omit values.

### 7.1 Source notice

Tokage is open source and does not upload data, but cannot stop third parties redistributing modified builds. UI reminds user to verify source at:

- First-run welcome (`WelcomeNotice`, before workspace trust dialog).
- CLI panel where Base URL / account shown (persistent fine print).
- Settings → About.

Shared `SourceNotice` component.

Suggested copy:

> Tokage reads Base URL and account type from local CLI config for display; it does not read API Key values, save, or upload them. Implementation: [src-tauri/crates/tokage-native/src/usage.rs](https://github.com/iwpz/tokage/blob/main/src-tauri/crates/tokage-native/src/usage.rs).
> These promises hold only for official releases or builds you compile from [github.com/iwpz/tokage](https://github.com/iwpz/tokage). Do not use installs from unknown sources.

Design notes:

- Source link to tag for version (`/blob/v{version}/...`); local build uses commit at build time, non-git build uses default branch. `build.rs` injects `TOKAGE_COMMIT` and `TOKAGE_BUILD_KIND`; `TOKAGE_OFFICIAL_BUILD=1` only in release pipeline → “Official release”. Frontend via `build_info`; open behavior per [10 §7](../10-frontend/README.md).
- About shows version, commit, build kind (official / local), official install verification: GitHub Releases SHA-256, macOS notarization Team ID, Windows signing subject. Placeholders until release channel fixed (P8.m6e).
- Limit: tampered copy can remove notice; app cannot prove integrity. Notice teaches verification; trust relies on official signatures and checksums.

## 8. Approval UI and audit

- Parallel task pending items in unified queue: task name, CLI / model, full command, parsed risk level and reasons, whether targets revertible.
- Actions: allow once / allow this session / add to project list / deny / deny with note.
- While waiting for approval, idle timeout paused for that turn ([09 §2](../09-guardrails/README.md)); hard timeout pause configurable.
- Each decision written to `approvals` ([08 schema](../08-storage/schema.md)).

## 9. Configuration

| Key | Default | Description |
|-----|---------|-------------|
| `settings.authMode` | `ask_sensitive` | Global: `ask_always` / `ask_sensitive` |
| `settings.authProjectModes` | `{}` | Per project id; unset inherits |
| `auth.full_auto_ttl` | `session` | `1h` / `today` / `session` |
| `auth.pause_idle_timeout_on_prompt` | `true` | |
| `sandbox.enabled` | `true` | Per-CLI disable only; disabled CLI cannot use `full_auto` |
| `credentials.redact_output` | `true` | Cannot disable |
