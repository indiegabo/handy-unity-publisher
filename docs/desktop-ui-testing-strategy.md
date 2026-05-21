# Desktop UI Testing Strategy

## Purpose

This document defines the testing strategy for the HGP desktop UI.

The goal is to make UI validation explicit, repeatable, and honest about what
has actually been verified.

## Current testing layers

The desktop UI now uses three practical validation layers.

### 1. Component tests

Tooling:

- `Vitest`
- `@testing-library/react`
- `jsdom`

Use this layer for:

- presentational primitives
- keyboard and focus behavior
- isolated field and overlay contracts
- component-level edge cases

Examples already in the repository:

- overlay stack behavior
- full-screen modal focus trap behavior
- compact interaction-focused editor tests

Command:

```bash
npm run test --prefix apps/desktop/ui
```

### 2. Screen integration tests

Tooling:

- `Vitest`
- `@testing-library/react`
- targeted service and Tauri API mocks

Use this layer for:

- screen-level workflows
- navigation between shell states
- overlays opened by real screen code
- assertions about what the operator can click, type, or dismiss

This is the correct layer for statements such as:

- "the worker quick-view flow worked"
- "the screen stayed on the main feed after Escape"
- "the picker returned data into the parent form"

Examples already in the repository:

- `apps/desktop/ui/src/App.test.tsx`

Command:

```bash
npm run test --prefix apps/desktop/ui
```

### 3. Browser-backed UI E2E

Tooling:

- `Playwright`
- a Vite E2E server with mocked Tauri APIs

Use this layer for:

- real browser clicks and key presses
- visual shell routing through the actual mounted application
- verifying that the UI still works when rendered and driven in a browser
  instead of only in `jsdom`

This layer is intentionally mock-backed.
It validates the desktop UI behavior without requiring a live Tauri host,
runtime process, or OS-native dialogs for every test run.

Commands:

```bash
npm run test:e2e --prefix apps/desktop/ui
npm run test:e2e:headed --prefix apps/desktop/ui
```

## Future testing layer

### 4. Host-backed desktop E2E

This repository does not yet ship a full host-backed desktop E2E harness for
the Tauri shell itself.

That future layer would be responsible for:

- window lifecycle behavior
- native file picker integration
- host auth flows
- real shell process behavior across the Rust boundary

Until that exists, browser-backed Playwright tests plus focused screen
integration tests are the correct combination for UI confidence.

## Native Tauri close-out expectations

For shell-visible changes, automated UI coverage is necessary but not
sufficient.

Run a host-visible Tauri pass with `npm start` whenever a slice changes:

- the main feed baseline or action bars
- focus-screen routing or Back behavior
- overlay precedence or dismissal behavior
- staged flows or draft guards
- long-content viewers
- visual hierarchy, density, or contrast of operator-facing surfaces

The goal is not broad exploratory QA. The goal is to confirm the specific
shell-visible contract touched by the slice in the real window.

Recommended source of truth for what to check: use the relevant items from
`docs/focus-screen-development-guide.md` and the historical close-out record in
`planning/focus-page-tasklists.md`.

## What each layer is allowed to claim

This distinction matters because test reporting should not lie.

### If a component test passes

Allowed claim:

- the component contract covered by the test worked

Not allowed:

- the whole screen worked
- the entire workflow worked

### If a screen integration test passes

Allowed claim:

- the screen or flow covered by the automated test worked

Recommended wording in reviews and task updates:

- `The UI flow covered by the test worked.`
- `The screen behavior covered by the automated test worked.`

### If a browser-backed Playwright test passes

Allowed claim:

- the browser-driven UI flow covered by the test worked end to end against the
  mocked desktop boundary

Not allowed:

- the full desktop shell worked against the real host

## Recommended authoring rules

- Prefer accessible selectors first: role, label, visible text.
- Test operator-visible outcomes, not implementation details.
- Keep one primary workflow assertion per test.
- Mock the Tauri boundary, not every component downstream of it.
- Add one focused browser E2E test only after the same flow has a stable
  screen-level contract.
- If a test passes for one flow only, report that flow as working, not the
  whole screen family.

## Recommended workflow for UI changes

For most UI work in this repository, validate in this order:

1. targeted component or screen integration test
2. focused desktop UI build
3. browser-backed Playwright flow for critical operator paths
4. native Tauri close-out pass when the change is shell-visible

Commands:

```bash
npm run test --prefix apps/desktop/ui
npm run build --prefix apps/desktop/ui
npm run test:e2e --prefix apps/desktop/ui
```

## Initial Playwright scope in this repository

The initial E2E scaffold covers mock-backed shell behavior such as:

- opening the worker quick-view from the main feed
- closing the overlay with `Escape`
- navigating from the home screen into a focus screen

That is enough to prove the harness is real.
It is not yet a substitute for a full host-backed desktop regression suite.
