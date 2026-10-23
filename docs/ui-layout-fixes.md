# UI cleanup acceptance

## ui1 — task, scope and CLI settings cleanup

- Task execution rows show the provider logo and model portion of the assigned spec; the full spec remains available on hover.
- In-scope and out-of-scope text wraps within its grid column, including long paths and unbroken strings.
- CLI settings no longer mount development reliability/milestone reports below readiness.
- Quota refresh is shown only for implemented official channels, disabled while querying, and publishes returned snapshots to both provider catalogs.
- Copilot uses its existing interactive CLI `/usage` command with cached fallback. The current parser preserves plan usage percentage, not absolute AI credit counts. No live quota/model request was run for this change.
- Validation: production build and 24 focused tests (provider store, reliability component, plan state, model contract).

Pipeline toolbar responsiveness is deliberately deferred pending approval of a separate layout proposal.

## cli-collapse-001 — collapsed CLI cards

- The card's automatic child divider is enabled only while expanded. Hidden details no longer leave a divider below the header; the outer card border remains.
- Validation: TypeScript typecheck and test-isolation static check passed. Native desktop visual acceptance has not been run for this change.
