# Shared runtime UI parity audit (sh4c-ui4)

## Cause and scope

The sh3 App migration routes native shared runs in `Workspace.tsx` into `RuntimeView` rather than the legacy Conversation/Planning/Pipeline presentation. The first runtime panel implemented native commands and raw output without preserving the existing presentation contract. Subsequent repairs reused the original message, plan and history renderers, but layout-only checks missed OS-native checkboxes, inconsistent action sizes and state-specific actions. This was a migration and verification omission; extracting the shared engine did not require replacing the application's visual language.

Reference: the retained `PipelineView` uses 28px small actions, Play/Square/Rotate icons, primary accent for starting/delivery, ghost secondary affordances and theme tokens. The shared runtime must preserve that presentation while submitting only native runtime commands. Legacy pause/resume semantics must not be copied onto CancelRun/ResumeRun: these have different durable meanings.

## Inventory and verification boundary

- Chat: original UserBubble/AssistantReply, Markdown, tools, task summaries; covered by component tests and previous manual observation. Pending final execution-time streaming observation.
- Plan: original SpecEditor/ContractsEditor/TasksTable, narrow headers, scrollable body, save/edit/model controls. Four viewport WebKit checks; final desktop preview observed by user. Runtime approval is an additional explicit native authorization step.
- Pipeline: task cards/table/details, status badges, assignment, retry/inspection controls. Existing component tests and four viewport WebKit checks. Legacy DAG scheduling/pause controls are not silently presented as native cancellation/reconciliation.
- Session: original history renderer backed by actual runtime transcripts, model/thread/usage and durable event ordering. Component coverage; final live executor/reviewer records pending.
- Approval dialog: unchecked/checked themed checkbox, keyboard/native input semantics, disabled until exact commands selected, explicit Full Auto confirmation retained. WebKit appearance/accent assertions and captured dialog screenshots at four widths; desktop observation pending.
- Execution settings: theme Switch for unknown-cost policy; themed bounded numeric/select controls; compact save and inspection actions. Existing settings overlay checks; desktop observation pending.
- Approved: compact primary Play start action. Offline WebKit dark/light state capture; real execution pending.
- Executing: compact Square cancellation and Refresh reconciliation actions; never label these pause/resume. Offline WebKit dark/light capture; real operation pending.
- Blocked/Stopped: compact retry and inspection actions. Offline WebKit dark/light capture; real fault-state acceptance pending.
- AwaitingMerge: compact primary GitMerge action, branch/patch alternatives. Offline WebKit dark/light capture; exact candidate inspection and normal UI merge pending.
- Native command approvals, takeover, amendments, terminal opening, patch destination and errors: retained controls and authorization handlers inspected; fresh real UI acceptance is pending. No test-only authorization or product state injection.

## Prevention and completion rule

Every shared-run UI change must list affected pages, states, dialogs and narrow layouts against this inventory, reuse existing presentation components/tokens, and preserve native command semantics. Run component regressions plus the external offline WebKit matrix; inspect dialog/state screenshots as well as geometry. Offline fixtures cannot prove desktop delivery. Record omissions as pending, preserve user-reported failures, and require normal desktop observation before marking sh4c complete. This matrix reduces omissions; it is not a claim that all possible states have already been accepted.

No model calls, approvals, execution or application database writes are part of this UI repair. The existing run remains awaiting approval. The user explicitly paused execution for this repair.

## sh4c-ui5 correction: structural parity

The user rejected the ui4 desktop preview: the pipeline had two competing headers and the chat still appended a temporary status/navigation card. ui4's passing geometry/color tests did not establish structural parity. Preserve that rejection rather than treating ui4 as desktop accepted.

The shared pipeline now has one board header containing its title/status/count, native actions, the original bordered Cards/Table segmented control with selected-state styling, and Back to review. Removed the separate padded background toolbar and duplicated board heading. The chat now uses the compact task board's header for original-style icon navigation to Plan and View pipeline DAG; the temporary Plan execution status card is removed. Both destinations still use the Workspace callback; no scheduler is mounted or duplicated. Task card surfaces use the existing `bg-elevated` token instead of undefined `bg-card`.

References inspected: `Conversation.tsx` PlanSubagentsBanner header/navigation (compact bordered icon links), `PipelineView.tsx` top control bar (one toolbar, selected Cards/Table control, Back to review). This restores their presentation patterns while preserving native command names and authorization.

Verification adds an explicit single-header/no-temporary-card regression and four real WebKit Chat screenshots with navigation icon and clipping assertions. Existing four-width Plan/Pipeline dialog/state scenarios remain. Desktop visual acceptance remains pending; do not resume execution until the user accepts this preview.

## sh4c-ui6: restore the shared bottom action panel

The user identified the original `PlanReview.tsx` Bottom Floating Actions & Review Control Bar as the intended location. Both shared-runtime Plan and Pipeline now render the same `RuntimeActionPanel` footer: task/contract summary and state at left, native run/settings/delivery actions at right. It follows the original rounded elevated surface, border, shadow, spacing and non-scrolling flex footer. Removed run controls from both top headers; page content scrolls separately. Chat navigation is unchanged. Run commands, Full Auto prompts and exact candidate confirmation remain unchanged.

Regression checks require a single footer after page content on both pages, bottom placement at all four widths, no approval action in headers, and primary state actions inside the footer for both pages/themes. The narrow contract preview uses reduced padding and remains independently scrollable when the footer is visible. Real desktop observation is still required before resuming execution.

## sh4c-ui7: explain blocked runs after completed tasks

The real App executor completed P0.M1a and both independent gates passed, but the CODEX_CLI Luna turn had null actual cost with blockUnknownCost enabled. The scheduler's next reserve-ready check rejects unknown actual turn cost before final review. All tasks were DONE, so the execution error handler's unfinished-task-only error projection discarded the run-level cause. No reviewer was dispatched; this is not a final delivery pass.

Persist optional `runError` in shared snapshots for execution/review/delivery failures even when no unfinished tasks remain. Clear it on leaving Blocked, retain it across read-only refresh/restart, and deserialize older snapshots without it. Display a visible blocked-reason banner in Plan/Pipeline; include delivery/planning/work errors previously omitted. Known missing-cost errors receive a localized explanation; older snapshots show recorded missing-cost/policy evidence, not fabricated cost or invented persisted error. Unrelated scope/gate errors must not be hidden by missing-cost diagnostics. No cost policy or acceptance gate is weakened.

The original PipelineView has a right-side selected-task detail panel. Retain that interaction, remove its duplicate state badge in the detail heading, and preserve the list card's badge. Offline WebKit captures the DONE-plus-Blocked state and asserts visible reason and no duplicate header badge. Real desktop observation remains pending.

## sh4c-ui8: chat contract hierarchy and streaming state

The user identified the chat answer's Contracts section (not task detail contracts). Previously code fences were independently labelled as contracts, and the ReactMarkdown renderer functions were recreated for every update, remounting stateful CodeBlock components and resetting expansion. Markdown renderers now have stable component identity; callback/default options are supplied through context. Parsed explicit Contracts headings group C1/C2 bodies into sibling collapsible cards, stopping at the next peer/ancestor heading. Fenced source text is not parsed as headings. Code inside a contract is shown as source with its copy action, without a second independently collapsed contract wrapper. User expansion/collapse persists during appended text.

421 frontend tests pass including actual React rerenders, changing callbacks, open/closed state retention and fenced-heading boundaries. External offline WebKit checks at 1280, 640 and 420px verify the same DOM node survives a partial fenced stream completing, C2 remains a sibling, criteria remain in C1 and source copy stays available. Screenshots reviewed; normal App observation remains pending. No model call, retry, cost-policy change or real App state injection.

## context-popover

Chat and task-message composers open context usage from a circular button below the input. The panel shows each agent's current context window, its token count and limit, the percentage used, and whether usage was reported or estimated. Windows with unavailable inputs show an unavailable state. The cache countdown has been removed from the UI. Viewing usage works with context actions turned off.

Frontend regressions cover separate task windows, measured zero, unavailable inputs, and popup dismissal. Offline WebKit checks covered the browser workspace and a task composer with two agent windows at widths of 1024, 640, and 420px.

The broader WebKit matrix required a temporary fixture export for the existing task-context import. With that export, both the baseline and the updated build failed the same approval checkbox color assertion.

On 2026-11-19, one isolated macOS release build of `cae35dd` ran GPT-6-Luna Medium through Codex CLI 0.161.0. A planner, two parallel executors, and a reviewer completed in four distinct native threads. A click on the circle opened all four rows. The accessibility tree confirmed the displayed counts, percentages, and source labels. The test also observed the circle beneath the task message input and the absence of the old top usage bar and cache countdown.

| Agent | Displayed tokens | Displayed usage | Source | Codex local log last input |
| --- | ---: | ---: | --- | ---: |
| Planner | 21,146 | 17% | CLI reported | 21,146 |
| Executor P0.M1a | 8,937 | 7% | Estimated | 22,882 |
| Executor P0.M1b | 8,941 | 7% | Estimated | 26,639 |
| Reviewer | 8,446 | 7% | Estimated | 27,342 |

The UI uses Tokage's effective window of 128,000 tokens. The planner's app-server receipt contained a per-window count. The three `codex exec` receipts contained cumulative input and omitted the per-window count. Each estimate matched an independent recomputation, and each was lower than the last-input count in its corresponding Codex log. The four-row display passed. Exact usage was verified for the planner.

Both task gates and final review passed. All six fixture tests passed on the independently checked-out candidate and on the delivered fixture. [Read-only verification and evidence hashes](reliability-evidence/context/context-popover-20261119.json).

### context-codex-usage

After a successful `codex exec`, Tokage checks the local rollout for the native thread that the CLI just reported. It accepts the last input token count when the completed turn matches the workspace, execution time, and aggregate counters in the exec receipt. A missing or conflicting rollout leaves the existing estimate available. The lookup reads local session telemetry, preserves the cumulative counters for accounting, and keeps the existing effective window limit for the percentage.
