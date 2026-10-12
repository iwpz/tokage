# 10 · Frontend Mission Control

**English** | [中文](README.zh-CN.md)

Back to [README](../../README.md)

## 1. Technology choices

| Item | Choice |
|------|--------|
| Framework | React 19 + TypeScript (strict) |
| Build | Vite |
| Styling | Tailwind CSS 4, dark default, light supported |
| Components | In-house lightweight system (`components/ui.tsx`, Tailwind 4; P5.m5a decided against heavy external UI libraries) |
| Routing | Zustand-driven desktop window state (no TanStack Router) |
| Server state | Hand-written Tauri IPC (`src/lib/ipc.ts`) + `DomainEvent` push (no TanStack Query) |
| Client state | Zustand |
| Task table & DAG | Dense multi-column task table and lightweight SVG state flow (React Flow deferred) |
| Markdown | `react-markdown` + `rehype-sanitize` |
| IPC types | Hand-written in `src/lib/ipc.ts`, 1:1 with Rust (why not `specta`: [00 §7](../00-architecture-overview/README.md)) |

> Current state (2026-10-05): P5.m5a reaffirmed the in-house lightweight design system (`components/ui.tsx`: Button, Switch, Select, NumberInput, Card, Badge, Tabs, Modal), avoiding React 19 compatibility friction and unnecessary runtime size; views and data flow driven compactly by Zustand and IPC, very fast builds and zero extra dependencies.

## 2. Layout

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ Top bar: [Claude ● 42%] [Codex ● 18%] [Cursor ●] [Aider ● $0.31]   Saved $12.40 this week  First-pass 91% │
├──────────────┬────────────────────────────────────────────────┬───────────────┤
│ Sidebar      │ Main area                                       │ Right (collapsible) │
│              │ ┌ User profile feature · Running · Est $0.92 / Actual $0.61 ┐ │               │
│ Project A    │ │ [Plan] [Pipeline] [Session]                          │ │ Selected task detail │
│ ├ User profile│ │                                                │ │  T2 frontend-ui │
│ └ Fix login  │ │   T1 backend-api ✓ ─┐                          │ │  Cursor Comp.   │
│ Project B    │ │   Codex  $0.18      ├─► T3 integration ● ─► T4 │ │  Confidence 0.88 │
│              │ │   T2 frontend-ui ✓ ─┘   Codex (reuse)     testing│ │  Est $0.21      │
│ External     │ │   Cursor $0.20                         ○ DS   │ │  Actual $0.20   │
│  (CLI native)│ │                                                │ │  Gate ✓ typecheck│
│              │ └────────────────────────────────────────────────┘ │  [Report] [Log] │
│ + Add project│ Composer  [Auto ▾ Plan/Direct]  [Target: main session ▾]   Send      │               │
└──────────────┴────────────────────────────────────────────────┴───────────────┘
```

## 3. Views

### 3.1 Plan review (`REVIEW` state)

- Left: spec and contracts (Markdown, editable).
- Right: task table—each row shows domain, dependencies, write scope, acceptance commands, assigned model, first-pass confidence, estimated cost; model column is a dropdown for reassignment; reassign recalculates the whole table.
- Bottom summary: total plan estimate, expected subscription quota use, expected savings vs reference model.
- Risk markers: tasks below confidence threshold, acceptance commands needing user confirm, write scope conflicts.
- Actions: confirm execution / request changes (incremental replan) / discard.
- After incremental replan confirm, `PlanDiffView`: task / contract add-change-delete; user confirms then apply diff and generate adapted tasks. When `requireReview` off or cost below `planningAutoApproveBelowUsd`, plans with no Error may auto-confirm.

### 3.2 Pipeline

- Task DAG: node color = state; model and cost on node; reuse windows shown with links; critical path highlighted.
- Switchable to kanban (columns: waiting / ready / running / verifying / done / blocked).
- Inline actions on blocked tasks: re-decompose / terminal takeover / skip.
- Top: pause / resume queue, actual vs estimated cost progress bars.

### 3.3 Session

- When a task or attempt is selected: log stream—brief (collapsible), assistant output, tool calls (folded), gate results, task report.
- Context bar: baseline / history / effective cap segments, cache TTL countdown.
- Context suggestion bar: action, estimated savings, reason; in suggest mode “Apply / Dismiss” writes `context_actions`; after compact apply, next turn carries compact payload and settles `CONTEXT` savings. While pipeline runs, Composer append messages are triaged (task queue / replan / direct dispatch).

### 3.4 Usage and quality dashboard

- Provider cards: quota ring (snapshot / inferred), reset countdown, cache hit rate, melt reason.
- Cost breakdown: planning / execution / repair share.
- First-pass rate matrix: model × domain, cell shows rate and sample count.
- Savings overview: ASSIGNMENT / CONTEXT / DIRECT, labeled “estimated”, expandable params.
- Token stacked area: uncached input / cache read / cache write / output.

### 3.5 Settings

Provider probe results, planning mode and planner model, confidence threshold, context mode, budget red lines, timeout multiplier, workspace isolation mode, reference model, pack version and updates, model overrides. Also:

- Permissions & security: auth mode (ask always / ask sensitive / full auto; full auto needs second confirm and TTL display) and per-project / current-session overrides, command and file-edit trial classification (level, reason, outcome under three modes), command list view and local rules, per-CLI sandbox and credential isolation capability (full / partial / unsupported). See [11](../11-command-authorization/README.md).
- Network proxy: global default + per installed CLI that can list models. See [12 §5](../12-network-proxy/README.md).
- Model capabilities: layered view of built-in / online / custom packs and field provenance. See [04 knowledge-pack §7](../04-model-matching-engine/knowledge-pack.md).
- About: version, commit, build kind, source link and install verification ([11 §7.1](../11-command-authorization/README.md)).

### 3.6 Approval queue

- Pending approvals as non-modal panel with count in top bar; each item shows task, CLI / model, command, risk level and reason.
- Actions: allow once / allow this session / add to project list / deny / deny with note; keyboard completable.
- Full auto mode: persistent warning bar in top bar.

## 4. State and data flow

```
Tauri Channel ──► eventRouter (singleton)
                    ├─ log_append              → streamStore (buffer by attemptId, rAF coalesce)
                    ├─ task / attempt_status   → setQueryData precise DAG node update
                    ├─ plan / assignment       → invalidateQueries(['plan', id])
                    ├─ context / suggestion    → contextStore
                    ├─ gate / cost             → setQueryData
                    └─ notice                  → toast queue
```

- First paint via Query fetch; thereafter incremental updates from events, no polling.
- Streaming text append-only, at most one refresh per frame; after complete, replace buffer with persisted content.
- After WebView reload, backfill from each window’s last `logId`.
- Cache countdown computed locally from `cacheWarmUntil`.

## 5. Shortcuts

`⌘K` command palette, `⌘N` new request, `⌘↵` confirm plan, `⌘.` pause queue, `⌘1/2/3` switch Plan / Pipeline / Session view.

## 6. Accessibility

- All interactions keyboard-accessible; DAG has equivalent table view for keyboard and screen reader users.
- State not color-only: nodes and status indicators use icons and text.
- Log stream uses `role="log"` + `aria-live="polite"`, announcing task complete, blocked, gate fail, and context suggestions—not every streaming token.
- Progress uses `role="progressbar"` with `aria-valuetext` (e.g. “Plan cost $0.61, estimate $0.92”).
- Contrast target WCAG 2.2 AA; respect `prefers-reduced-motion`.
- Before release: VoiceOver / NVDA testing and human review.

## 7. Security

- Markdown via `rehype-sanitize`, raw HTML forbidden.
- Links open in system browser via Tauri opener; reject non-http(s) schemes.
- CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost`.
- Frontend cannot execute commands or access filesystem directly.
- CLI panel Base URL shows source reminder and source link ([11 §7.1](../11-command-authorization/README.md)).

## 8. Performance targets

| Metric | Target |
|--------|--------|
| Cold start to interactive | < 1.5 s |
| 20-task DAG live update | 60 fps |
| Single window 100k log lines scroll | 60 fps |
| 4 execution windows streaming | main thread < 8 ms per frame |
| Rust process idle resident memory | < 50 MB |
| Overall (incl. WebView) idle resident | < 150 MB |
