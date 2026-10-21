# Demo mode

**English** | [中文](README.zh-CN.md)

Enable **Settings → Planning & models → Demo mode**. The sidebar gains `demo_project` / `demo_work`. Open the demo to see the seeded request and Fable 5.1 plan, review the plan, then confirm execution. The actual output source is explicitly Mock; no provider CLI installation or login is required.

## Pocket Tasks script

The isolated JavaScript starter gains completion, filtering, remaining counts and persistence:

1. T1: data model and persistence.
2. T2: operations and filtering, after T1.
3. T3: views and counters, after T1.
4. T4: persistence regressions, after T1.
5. T5: integration, after T2 and T3.
6. T6: workflow regressions, after T4 and T5.
7. T7: review and delivery notes, after T6.

T2–T4 can run in parallel within the configured concurrency limit. Planning, validation, model allocation, context, scheduling, reports, repair, Git and delivery use existing implementations. Demo catalogs work independently of real provider installation and trust; ordinary projects retain their own settings.

Requests, plans, task descriptions, report bodies, errors and gate feedback support English and Chinese. Task identities, dependencies and code are shared. Machine report headings and the `None` sentinel remain fixed as required by the existing report protocol.

## Scenarios and controls

- **Success:** complete all seven tasks and enter the existing delivery flow.
- **Repair:** T2 initially writes an incorrect remaining count. Mock acceptance fails, the normal repair path writes the corrected implementation, and acceptance passes.
- **Execution error:** T3 returns a localized error, exercising normal failure handling and downstream blocking.
- **Slow:** T3 waits 30 seconds, allowing cancellation and continuation checks; other tasks run normally.

Choose a scenario, then create/reset the demo to apply it. Each session retains its creation-time scenario and language. Change application language and reset for the other language. Stream interval changes affect subsequent turns.

Disabling demo mode hides the project while preserving its records. Existing running tasks continue through Mock. Reset creates a fresh run and replaces the visible demo sessions; active execution or verification must finish or be cancelled first. Old repositories remain on disk for inspection.

## Boundaries and checks

Desktop runs create real repositories under `debug-workspaces/demo/<run-id>/demo_project`, with real files, worktrees, commits and merges. User repositories are untouched. Git is required. Final branch delivery uses existing controls.

**Demo acceptance feedback is simulated; it does not execute `npm test`.** The produced project supports local `npm test`, and the integration regression test actually runs `node --test` on the merged output. Browser preview uses in-memory files and simulated Git.

Project, conversation, plan, attempt and gate records use the existing application database, with explicit Mock attribution. Demo model calls do not create production usage, learned-success or savings records. Both persisted session identities and a tracked workspace marker enforce Mock in the backend, even after debug settings change or demo mode is disabled. Missing isolation fails instead of launching a provider.

The workspace check panel uses events observed in the current app process to check Mock routing, zero reported usage, one end event per ended turn, and dependency timing. Unobserved checks stay unchecked; restarting begins new event observations. These checks do not certify model ability or provider CLI compatibility.

There are currently no scripted approval, quota switching, merge-conflict, malformed-plan or incremental-feature scenarios. Existing interactions remain available, but are not claimed as covered branches.

## Verification

```sh
npm run typecheck
npm test
npm run build
npm run check:i18n
cd src-tauri
cargo test -p tokage-native --lib chat::debug
cargo test -p tokage-native --lib chat::demo
cargo test -p tokage --lib debug_provider
```

These commands do not enable paid live tests. Frontend tests cover bilingual initialization, reset, route isolation, planning and task execution without real providers, dependencies, repair, errors and cancellation. Rust tests cover enforced Mock routing, real parsing and allocation, bilingual report parsing, worktrees, commits, merges and real local tests of the final output.

TypeScript and Rust share `src-tauri/resources/demo/pocket-tasks.json`. Keep both language versions and machine protocol fields consistent when extending scripts.

## Older runs blocked at delivery (demo-delivery-001)

`Acceptance plan changed; apply the edited plan through replan before delivery` means the saved plan differs from the executed definition. Older builds could write the original model Markdown after assigning models or normalizing scopes, losing those changes on reload. Current persistence renders the approved structured plan and saves a matching JSON snapshot; approval also records its identity before execution.

Restart the updated desktop app (including its backend), then use **Reset demo** and approve the new run. Reloading only the frontend does not update the native plan writer. Existing runs without an approved plan identity cannot be certified retroactively. If you actually edited a running plan, apply it through replan.

Regression coverage now waits for successful full-plan delivery in both success and repair scripts, checks that subsequent plan edits are blocked, and round-trips both localized demo plans through native persistence after scope/model changes. These are automated unit/integration checks, not external desktop UI acceptance.
