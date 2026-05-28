# Focus Screen Development Guide

## Purpose

This document tells contributors and AI agents how to extend or refactor the
HGP desktop UI now that the focus-screen delivery program has landed and passed
its native Tauri close-out checks.

Use this guide for work under `src-react` that touches:

- route-level screens
- shell navigation in `App.tsx`
- overlays and viewer flows
- staged or wizard-like transactions
- dense operator-facing section layouts

For the original rationale and acceptance history, see:

- `planning/ui-focus-mental-model.md`
- `planning/focus-screen-ui-hierarchy-plan.md`
- `planning/focus-page-tasklists.md`

## Shipped UI Model

The desktop shell now has one stable interaction model.

- The main feed is the dispatch board. It routes the operator into work, but it
  is not another focus screen.
- A focus screen is the route-level working context for one operational task.
- A full-screen overlay is a blocking subtask that resolves to a typed result
  or `null`.
- A lightweight overlay is a short-lived decision surface that keeps context
  visible.
- `StepFlow` is the staged transaction primitive for ordered multi-step work.

## State Ownership Rules

- The shell owns the navigation stack, overlay stack, and dismissal precedence.
- A focus screen owns its draft state, inspection state, and local reload or
  retry behavior.
- An overlay owns only the temporary state required to complete its subtask.
- An overlay must return a typed result or `null`; it must not mutate parent
  draft state through hidden shared state.

## Mandatory Construction Rules

### 1. Choose the correct surface

- Keep the main feed as the dispatch board.
- Use a focus screen for route-level editing, inspection, or management work.
- Use a full-screen overlay for pickers, credential composition, destructive
  confirmation with real context, long logs, large lists, and artifact
  inspection.
- Use `StepFlow` when the operator must complete ordered decisions with local
  transition guards.

### 2. Reuse the shipped primitives first

Prefer these building blocks before inventing one-off wrappers:

- `ScreenScaffold` for page identity, summary, and action layout
- `SurfacePanel`, `MetaRow`, and related summary primitives for section grammar
- `OverlayManager` and `openOverlay` for blocking subtasks
- `FullScreenModal` for overlay structure and focus behavior
- `InputWithPicker` plus `SelectListFullScreen` for picker-backed fields
- `ConfirmDialog` for destructive confirmation
- `LogViewerModal` and `ArtifactViewer` for heavy inspection content
- `StepFlow` for staged flows

If a new primitive is genuinely required, keep it reusable and document why the
existing surface family could not express the need.

### 3. Preserve dismissal precedence

The shell must continue to resolve dismissal in this order:

1. top-most overlay
2. current focus screen
3. root-level shell close or exit behavior

This rule applies to `Back`, `Escape`, explicit close actions, and any future
shell-controlled close request.

### 4. Keep support content secondary

- Support callouts, warnings, and guidance may be visible and important.
- They must not outrank the active task region.
- Dense screens should answer page identity, task purpose, and the next safe
  action before the operator reads the first field.

### 5. Preserve the dense focus-screen grammar

- Use `ScreenScaffold` to establish page identity.
- Keep major functional slices inside clear section containers.
- Use summary rows and metadata strips instead of badge clouds.
- Keep nested editors visually subordinate to the parent section.
- Preserve the established compact dark theme and avoid marketing-style empty
  space.

### 6. Cover full feedback states

Every touched screen should define:

- loading
- refreshing when relevant
- empty
- error
- stale or partial data
- pending local action

Long-running actions must expose ownership without freezing unrelated content.

### 7. Preserve accessibility contracts

- Opening an overlay should move focus to the first meaningful interactive
  element.
- Focus should be trapped while the overlay is active.
- Focus should return to the invoking control on close whenever possible.
- Keyboard dismissal and traversal should remain explicit and predictable.
- Accessible names must stay stable for headers, modal titles, icon-only
  controls, and destructive actions.

## Validation Rules

Treat every UI slice as incomplete until validation has passed.

### Automated validation

Start narrow and escalate only when the surface requires it.

1. Run the focused component or screen integration test for the touched slice.
2. Run `npm run build --prefix src-react`.
3. Run the relevant browser-backed Playwright flow when shell routing or a
   critical overlay path changed.

Use `docs/desktop-ui-testing-strategy.md` for the allowed claims of each test
layer.

### Native Tauri close-out pass

When a change is shell-visible, also run the relevant host-visible checks in a
real Tauri session started with `npm start`.

At minimum, cover the items affected by the change:

- main feed baseline and action bars if the dispatch board changed
- overlay dismissal precedence if an overlay path changed
- Back behavior if focus-screen routing changed
- viewer readability and focus restoration if a heavy inspection surface changed
- staged-flow cancel or return behavior if a wizard or multi-step flow changed
- contrast and density if visual hierarchy or action layout changed

Do not claim host-backed shell validation from jsdom or mock-backed Playwright
alone.

## Common Anti-Patterns To Reject

- Ad-hoc popovers that bypass `OverlayManager`
- New route-level screens that ignore `ScreenScaffold`
- Overlays that mutate parent state without returning a typed result
- Heavy logs, large lists, or artifacts rendered inline by default
- Support callouts that visually compete with the primary editor or viewer
- Badge-heavy layouts used to compensate for weak hierarchy
- Reporting browser-backed or integration-test coverage as if it were real
  host-visible shell validation

## Definition Of Done For New UI Slices

Before calling a desktop UI feature or refactor complete, confirm that:

- the surface fits the shipped dispatch-board/focus-screen/overlay model
- shared primitives were reused before new wrappers were introduced
- dismissal precedence and focus restoration still hold
- loading, empty, error, stale, and pending states remain honest
- focused automated validation passed
- the relevant native Tauri close-out checks passed for shell-visible changes

If that bar is not met, the slice is not done.
