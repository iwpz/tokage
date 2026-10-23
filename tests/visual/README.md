# Offline runtime layout regression

This harness renders production components against a test-only transport and static fixture.
It cannot dispatch model requests, inspect real worktrees, approve a real run, or access the App database.
It is **not** external-ui-v1 desktop acceptance and is not loaded by production entrypoints.

Install Playwright and WebKit in an external validation directory. Keep dependencies and browser
binaries outside the repository; record versions, cache location and install logs with the results.
`PLAYWRIGHT_BROWSERS_PATH` applies only to this offline browser runner, never the product App.

The fixture server uses a separate cache under the OS temporary directory, distinct from the product dev server cache.

Start the fixture server from the repository:

```sh
node_modules/.bin/vite --config tests/visual/vite.config.ts
```

Then run (substitute your external absolute paths):

```sh
PLAYWRIGHT_BROWSERS_PATH=/absolute/visual-tools/browsers node tests/visual/runtime-layout.mjs \
  /absolute/visual-tools/node_modules/playwright/index.mjs /absolute/layout-results
```

The runner checks WebKit layout at 1280×820, 1024×700 640×480 and a 420×480 workspace column, including plan body height,
visible Spec/Contracts/Tasks content, scrolling, horizontal overflow, first-screen pipeline cards,
settings overlays and exact-command approval controls. It saves screenshots and measurements.
The final approval button is never clicked. Stop the owned fixture server afterwards.

The sh4c-ui4 matrix additionally captures unchecked/checked approval dialogs, asserts
appearance-independent themed checkbox color, and checks 28px icon actions for Approved,
Executing, Blocked, Stopped and AwaitingMerge in dark/light themes. Reduced motion freezes
color transitions for deterministic screenshots. These static states never authorize a real run.
The parity inventory and remaining desktop gaps are in `docs/shared-runtime-ui-parity.md`.

sh4c-ui5 also covers Chat at all four widths, checking Plan/pipeline icon navigation
inside the existing board header and absence of the temporary duplicate status card.

sh4c-ui6 requires the shared action footer to remain at the workspace bottom on both
Plan and Pipeline, with run actions absent from top headers. State/theme captures
cover both pages; narrow contract content may be scrolled within its own preview.
