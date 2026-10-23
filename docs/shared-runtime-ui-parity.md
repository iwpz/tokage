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
