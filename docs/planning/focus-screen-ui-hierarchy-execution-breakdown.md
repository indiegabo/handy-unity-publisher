# Focus screen UI hierarchy execution breakdown

## Purpose

This document breaks the focus-screen hierarchy and overlay initiative into
implementation-ready work packages for the desktop UI.

Each package is intentionally narrow.
It names the target surfaces, states what must not change, and defines the
minimum validation step required before the next package starts.

## Scope lock

- the main feed remains an accepted visual baseline
- shell-level overlay governance and back handling in `App.tsx` are in scope
  when required by focus-screen behavior
- runtime contracts, Tauri command shapes, and repository workflow logic are
  out of scope unless a slice cannot be completed without thin shell
  adjustment
- this breakdown is for focus-screen delivery, not broad UI redesign work

## Delivery rules

- prefer extending shared primitives before adding one-off wrappers
- keep the compact dark operator-facing style
- do not widen the work package once implementation has started
- validate each package with `npm run build --prefix src-react`
- when a package introduces a new overlay contract, add focused automated
  coverage if the surrounding test harness allows it

## Recommended execution order

1. contract alignment and shared foundations
2. shell overlay governance
3. high-ROI focus screens: projects list and repository editor
4. staged and credential-heavy flows
5. heavy inspection surfaces
6. consistency, accessibility, and validation sweep

## Work package 0 - Contract alignment

### Objective

Remove contradictions between planning documents before implementation keeps
drifting.

### Target files

- `planning/focus-page-tasklists.md`
- `planning/ui-focus-mental-model.md`
- `planning/focus-screen-ui-hierarchy-plan.md`
- `planning/focus-screen-ui-hierarchy-execution-breakdown.md`

### Tasks

- align scope language so the main feed remains visually stable while shell-
  level overlay behavior in `App.tsx` is still allowed
- state one consistent overlay result contract across all documents
- define one shared definition of done and validation rhythm

### Out of scope

- code changes in this package

### Done when

- planning documents no longer disagree about scope or ownership
- implementation work can refer to one consistent vocabulary

### Validation

- manual document review for scope, terminology, and sequence consistency

## Work package 1 - Shared overlay and scaffold foundations

### Objective

Stabilize the primitives that all later screens depend on.

### Target files

- `src-react/src/components/OverlayManager.tsx`
- `src-react/src/components/FullScreenModal.tsx`
- `src-react/src/components/ScreenScaffold.tsx`
- `src-react/src/components/InputWithPicker.tsx`
- `src-react/src/components/FullScreenFileBrowser.tsx`
- `src-react/src/components/OverlayManager.test.tsx`
- `src-react/src/components/FullScreenModal.test.tsx`

### Tasks

- guarantee promise-based overlay opening and typed result resolution
- guarantee focus trap, scroll lock, and focus restoration semantics
- guarantee page header and content rhythm can be expressed through one shared
  scaffold
- guarantee picker fallback flows work without forcing inline complexity into
  calling screens

### Out of scope

- screen-specific visual polish beyond what is required to prove the shared
  primitives

### Done when

- focus screens can open blocking subtasks without ad-hoc modal code
- the shared scaffold can express page identity and actions consistently
- tests cover the base overlay behavior contract

### Validation

- `npm run build --prefix src-react`
- relevant unit tests for overlay foundations

## Work package 2 - Shell overlay governance and back precedence

### Objective

Make dismissal rules deterministic at the shell root.

### Target files

- `src-react/src/App.tsx`
- any shell-level navigation helpers or overlay host glue used by `App.tsx`

### Tasks

- identify every overlay or popover entry point currently governed outside the
  shared overlay system
- ensure overlay dismissal outranks focus-screen pop
- ensure root-level exit handling only runs after overlays and focus screens
  are exhausted
- migrate `WorkerStatusIndicator` or similar shell-level quick actions to a
  governed overlay path

### Out of scope

- visually redesigning the main feed
- changing business actions exposed by the main feed

### Done when

- the shell root follows one dismissal rule for close, Back, and Escape
- global overlays no longer bypass the overlay manager

### Validation

- `npm run build --prefix src-react`
- focused integration coverage for Back-closes-overlay-first behavior when the
  test harness makes it practical

## Work package 3 - Projects list upgrade

### Objective

Use the projects list as the first complete proof that the new hierarchy model
improves scan speed without reducing density.

### Target files

- `src-react/src/components/ProjectsFocusScreen.tsx`
- shared project-list child components under `src-react/src/components`
- `src-react/src/styles.css` only if shared tokens still need extension

### Tasks

- strengthen page header and dominant list section ownership
- preserve repository identity as the first scan anchor
- demote secondary facts into consistent metadata rows
- ensure quick actions or selection flows use managed overlays where they help
- verify loading, empty, refresh, and error states still read as one coherent
  page

### Out of scope

- changing repository data shape
- inventing new project-management features

### Done when

- entries can be scanned vertically without reading every chip
- quick actions feel attached to the list workflow, not bolted onto it

### Validation

- `npm run build --prefix src-react`
- focused test or manual QA path covering at least one overlay-return flow

## Work package 4 - Repository editor and publish destination ergonomics

### Objective

Improve the largest editing surface first, then carry the same discipline into
publish destination editing where inline complexity is currently highest.

### Target files

- `src-react/src/components/RepositoryProjectDetail.tsx`
- `src-react/src/components/PathPickerField.tsx`
- `src-react/src/components/PublishDestinationsEditor.tsx`
- `src-react/src/components/RepositoryCredentialComposer.tsx`
- new supporting form components under `src-react/src/components/forms`

### Tasks

- preserve page-level identity and save ownership
- clarify section summaries and nested editor ownership
- move credential composition into overlay-based subflows
- move large target binding or picker work into managed selection surfaces
- preserve validation, dirty-state, save, and reload behavior exactly

### Out of scope

- runtime store changes
- changing project or publish data contracts

### Done when

- the editor is easier to scan and navigate without changing what save means
- nested editors are clearly children of their parent sections
- credential and binding complexity no longer depends on cramped inline UI

### Validation

- `npm run build --prefix src-react`
- focused form or integration tests for save behavior if present
- manual QA for long-form keyboard navigation and cancel safety

## Work package 5 - Wizard and auth flow staging

### Objective

Make staged flows explicit, recoverable, and overlay-aware.

### Target files

- `src-react/src/components/CreateProjectWizard.tsx`
- `src-react/src/components/AuthProvidersFocusScreen.tsx`
- new flow helpers under `src-react/src/components/wizard`
- new auth overlays under `src-react/src/components/auth`

### Tasks

- implement or adopt `StepFlow` for staged transitions
- ensure auth or credential overlays return typed results into the parent flow
- keep support content secondary to the active step
- define cancel, close, retry, and review behavior explicitly

### Out of scope

- changing repository creation semantics
- redesigning provider back-end integration

### Done when

- the wizard reads like one transaction instead of loose panels
- auth binding behaves like a first-class subflow with safe cancellation

### Validation

- `npm run build --prefix src-react`
- focused integration coverage for one successful auth or credential return path
- manual QA for step transitions and dismiss behavior

## Work package 6 - Project workers operational hierarchy

### Objective

Preserve density while clarifying which status belongs to the runtime as a
whole and which status belongs to a specific worker or project group.

### Target files

- `src-react/src/components/ProjectWorkersFocusScreen.tsx`
- supporting worker components under `src-react/src/components/workers`

### Tasks

- extract runtime toolbar and worker group components where needed
- separate runtime-wide summary from per-worker sections
- move destructive or bulk controls behind governed overlays
- define empty, stale, and partial data states clearly

### Out of scope

- changing runtime behavior
- changing worker payload shapes

### Done when

- runtime summary and worker inventory no longer compete visually
- destructive controls have explicit confirmation paths

### Validation

- `npm run build --prefix src-react`
- focused manual QA for destructive-action cancellation and successful bulk
  action flow

## Work package 7 - Process detail heavy viewers

### Objective

Move large inspection content into dedicated viewers without weakening the base
outcome screen.

### Target files

- `src-react/src/components/ProcessDetailFocusScreen.tsx`
- new modules under `src-react/src/components/process`
- `src-react/src/components/LogViewerModal.tsx`

### Tasks

- extract process detail panels into smaller modules
- move long logs into a dedicated viewer overlay
- add artifact viewing and download affordances
- ensure cleanup or retention actions use explicit confirmation

### Out of scope

- changing process data contracts
- building a full artifact browser beyond the needs of the current screen

### Done when

- long logs are no longer rendered as heavy inline blobs by default
- artifacts and outputs can be inspected without losing the surrounding process
  context

### Validation

- `npm run build --prefix src-react`
- focused manual QA for copy, select, download, and dismiss behavior

## Work package 8 - Consistency, accessibility, and test sweep

### Objective

Remove leftover divergence after the screen-specific slices land.

### Target files

- `src-react/src/styles.css`
- any touched focus-screen component that still diverges from the shared model
- overlay or screen tests that still miss critical behavior

### Tasks

- normalize title spacing, metadata rows, and chip tone usage
- remove local overrides that duplicate the shared hierarchy model
- verify keyboard entry, focus restoration, and screen-reader naming
- confirm no focus-screen-specific styling leaked into the main feed
- add missing integration coverage for representative overlay families

### Out of scope

- new feature work unrelated to consistency or validation

### Done when

- focused screens share one visual and interaction grammar
- the main feed remains visually stable
- required build and test checks pass for the touched surfaces

### Validation

- `npm run build --prefix src-react`
- relevant unit and integration tests
- concise manual QA checklist for keyboard, contrast, density, and dismiss
  behavior

## Recommended execution rhythm

- keep each package small enough to review in one pass
- run validation immediately after each package lands
- avoid mixing foundation work with multiple unrelated screen rewrites in one
  change
- prefer landing shared primitives before styling around their absence
