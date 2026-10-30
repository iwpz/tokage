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
- SA2–SA4 pending.

Real-model acceptance must use normal desktop UI and external observation under
`docs/testing-isolation.md`. Unit/fixture results do not establish desktop or live
model success.
