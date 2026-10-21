# AGENTS.md

<!-- tokage:begin -->
## Tokage project context

This project is worked on through Tokage. Shared context lives in `.tokage/`:

- `.tokage/context.md`: constraints and task state. Read it before starting work.
- `.tokage/plans/<session-id>.md`: the current plan of each Tokage session. When a prompt names a plan file, follow it.

Do not edit files under `.tokage/` unless asked; Tokage maintains them.
<!-- tokage:end -->

## Development workflow

Follow [.cursor/rules/subnode-commits.mdc](.cursor/rules/subnode-commits.mdc): automatically commit each completed and validated development subnode, including its identifier in the commit message.

Follow [.cursor/rules/test-isolation.mdc](.cursor/rules/test-isolation.mdc) and [docs/testing-isolation.md](docs/testing-isolation.md) before test infrastructure changes or real-model acceptance. Starting with m3a-010, tests must use external normal-UI observation; do not restore internal acceptance drivers or runtime test exceptions.
