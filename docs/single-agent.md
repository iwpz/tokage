# Single-agent work

Direct work is a first-class work style. A complex request does not implicitly
create a Tokage plan. Planning remains an explicit choice, with either per-task
allocation or a fixed model. Resource choice is independent of the work style.

## Implementation contract

- SA1: default to direct work; separate planning resource strategy; migrate old
  implicit Auto defaults to Direct, preserving explicit Plan/Fixed choices.
- SA2: durable direct-turn execution, one owner per session/workspace, crash-safe
  launch and recovery. Reuse native adapters without inventing a one-task plan.
- SA3: recommend locally for a new session; continue the existing resource on
  follow-ups; no silent failover or paid planning call for routing.
- SA4: explicit resource handoff and linked plan upgrade, preserving the source
  conversation and completed work.

## Boundaries

Direct work retains the existing CLI permissions. Codex ordinary direct turns
are read-only; its write-capable mode still requires explicit Full-auto and a
linked worktree. Cursor configurations that permit unapproved execution remain
excluded. File editing, tool interaction and approval support vary by CLI.
Unknown cost is not zero. Stopping, completion and verified checks are distinct.
No automatic commit, merge, rollback, vendor failover or request replay is added.

## Progress and evidence

- SA1 implemented: two primary composer work styles; fixed-model planning is a
  resource strategy; manual model selection remains explicit even when it equals
  the recommendation. New installs default to direct, and settings v8 migrates
  old Auto defaults. Fixed planning never triages back to Direct.
- SA1 validation: TypeScript check; 19 focused routing/settings tests; i18n and
  test-isolation checks passed. Desktop and real-model acceptance not performed.
- SA2 implemented: transport-independent native direct execution service; durable
  request/event journal; shared session ownership with plan runtime; canonical
  workspace write lease; explicit unknown-outcome reconciliation. Desktop history
  can read saved replies even if a batched UI snapshot was lost. Direct work uses
  the configurable agent timeout tier and preserves pending approvals.
- Public CLI shares the service through `direct send/history/events/reconcile`;
  workspace trust and Full-auto require separate explicit flags.
- SA2 validation: native suite passed 441 tests plus runtime-default regression
  (34 paid/installed-CLI tests ignored); CLI consent, offline protocol journal,
  duplicate refusal and cancellation tests; runtime regressions and desktop
  compilation. No real model or external desktop acceptance has been performed.
- Initial native run: 440 passed, one PTY trust fixture failed; focused rerun also
  failed, while the unchanged committed baseline passed once. The fixture wrote
  its marker even when `read` ended on EOF; it now writes only after a successful
  line read. Initial runtime suite also exposed a Claude protocol fixture relying
  on real local auth; the test-only environment now supplies a fake API key to the
  explicit fixture executable. Initial failures remain recorded in this report.
- SA3 implemented: one local resource selector drives preview and dispatch; the
  active session keeps its previous resource, even after another session changes
  its selection. Unavailable manual/previous resources require an explicit choice.
  Resource preferences persist via ordinary settings rows, including stable Auto.
  Legacy queued Auto messages migrate to Direct. Fixed-model planning does not
  silently replace an unavailable explicit model.
- SA3 validation: local selection/continuity/unavailability/failure tests, direct
  dispatch tests without triage or per-turn assignment, and metadata round-trip
  tests. The exact staged frontend is typechecked and regression-tested separately.
- SA4 implemented: switching to a different resource requires an editable handoff
  review. The next ordinary direct turn uses a fresh window and the reviewed brief
  instead of replaying all history. Stale/active/approval/compaction states reject
  acceptance, and a handoff suppresses automatic report/compaction model turns.
  Pending handoffs and source links persist through ordinary metadata rows.
  Approval receipt failures do not grant or resume; resolving one approval never
  rewrites earlier failed turns. Disabled resource controls retain the actual
  CLI/model label.
- Explicit plan upgrade creates a separate linked session, keeps the original
  conversation and existing work, checks unresolved native execution and ordinary
  clean/attached Git requirements, and invokes planning only. It never starts plan
  execution or commits/discards existing changes. The planning resource is shown
  and selectable in the review dialog.
- SA4 validation: handoff bounds/provenance tests, normal send-path tests with no
  native resume or full-history replay, stale/approval rejection, persistence
  round-trip, and linked upgrade/dirty/unresolved checks. Frontend typecheck,
  complete unit suite, i18n, isolation check and production build passed.
- Final automated results: frontend 597 tests; native 442 unit tests plus the
  runtime-default test, with 34 installed/paid cases ignored; shared runtime 61
  tests plus the added cross-engine ownership regression; public CLI 5 tests
  covering consent, offline output persistence, duplicate refusal, pending
  approval without Done, and cancellation. The final direct-service checks also
  verify immutable prepared input and unknown-PID reconciliation refusal.
  Desktop Rust compilation passed. Build retains existing chunk-size and mixed
  static/dynamic import warnings.

## Remaining acceptance and deliberate limits

### Session recovery follow-up

- SR1: native closed-session snapshot, ordered durable turn links, partial-tail
  disclosure, active-owner fencing, approval-audit readback, project discovery,
  and stale-UI tip checks before accepting another request.
- SR2: reconstruct chat messages/tools/files/usage and safe native continuation,
  restore missing sidebar sessions through normal UI, hydrate existing sessions,
  and prevent automatic resend, approval grants or queue drain during recovery.
- SR1 native regression: seven direct-service tests passed, including active
  owner refusal, interrupted partial journals, malformed closed history, same-second
  ordering and stale continuation rejection. Desktop compilation and the
  test-isolation check passed.
- SR2 implementation pending. No real-model/desktop recovery acceptance yet.

- Real paid-model execution and external normal desktop UI acceptance have not
  been performed for this feature. Automated and protocol-fixture evidence is
  recorded separately and must not be presented as desktop acceptance.
- The initial local recommendation reuses the existing model catalog and ranking;
  this change does not introduce new subscription/cost/ability preference presets.
- Handoff extracts are local and editable, bounded to recent messages and reported
  changed paths; omissions are explicitly labeled. Hidden native state is not
  transferred. Large existing histories are not silently summarized by another
  paid model.
- Direct CLI transport returns saved events at completion rather than offering an
  interactive terminal tool/approval bridge. Desktop retains its normal event UI.
- Native records preserve intent and output independently of the UI's 300 ms batch
  snapshot; saved replies are inspectable in execution history. An interrupted
  operation still requires explicit reconciliation and workspace inspection.
- On Unix, direct inference records a pre-spawn intent and PID in a per-turn process
  directory; reconciliation refuses the intent-to-PID crash gap. Native error
  frames remain provisional until process exit, preserving Stop during cleanup.
  Automatic reconciliation of interrupted direct turns is unsupported on Windows;
  ordinary completed turns retain the existing process-registry semantics.
- Final frontend validation uses Node 24.12.0. A direct Vitest startup under Node
  22.16.0 in an isolated snapshot failed before tests with a `styleText` argument
  error; no project or global runtime configuration was changed.

Real-model acceptance must use normal desktop UI and external observation under
`docs/testing-isolation.md`. Unit/fixture results do not establish desktop or live
model success.
