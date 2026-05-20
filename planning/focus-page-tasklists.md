# HGP focus UI delivery program

## Purpose

This document is the master execution document for the focus-screen initiative
in the HGP desktop UI.

It exists to define:

- what the UI must become, not only which components must be created
- which interaction contracts are mandatory across screens
- which parts of the shell are in scope, out of scope, or conditionally in
  scope
- how work should be sequenced so the effort remains reviewable and testable

The companion documents support this one:

- `planning/ui-focus-mental-model.md` explains the interaction philosophy and
  behavior contracts
- `planning/focus-screen-ui-hierarchy-plan.md` defines the visual hierarchy
  grammar
- `planning/focus-screen-ui-hierarchy-execution-breakdown.md` slices the work
  into implementation-ready packages

## What we are actually building

This initiative is not a cosmetic modal pass.

It is a workflow refactor that turns the current set of dense focus screens
into a coherent operator navigation system with:

- explicit push/pop flow for task-oriented screens
- overlay-based interruption for pickers, confirmations, and viewers
- stable page, section, and item hierarchy across the shell
- deterministic Back, Escape, and close semantics
- readable high-density inspection and editing surfaces

The target outcome is a desktop shell that feels like one disciplined local
operations tool rather than a pile of unrelated forms and panels.

## Outcome statement

When this program is complete, an operator should be able to:

- move from the main feed into any focus screen without losing orientation
- understand the purpose of a screen from its page header alone
- distinguish page, section, and item boundaries without reading every label
- complete picker, credential, and confirmation subtasks through managed
  overlays with explicit cancel or confirm results
- inspect long logs, retained outputs, and worker inventories without bloating
  the base screen with unreadable inline content
- trust that Back, Escape, and close actions behave consistently everywhere

## Scope lock

In scope:

- focus screens and their supporting components under
  `apps/desktop/ui/src/components`
- shell-level overlay orchestration and back handling in
  `apps/desktop/ui/src/App.tsx`
- page, section, and item hierarchy across existing focus screens
- overlay result contracts, staged flows, and heavy-viewer surfaces
- tests that prove overlay result flow, focus management, and screen-level
  regressions
- thin Tauri or shell glue only when required to preserve the current UI
  behavior contract

Out of scope:

- a full visual redesign of the main feed
- runtime orchestration, store contracts, or command payload redesign
- speculative mobile or web layouts outside the desktop shell
- new business workflows that are unrelated to existing focus-screen behavior

## Non-negotiable experience principles

- The UI stays dense, tool-like, and operator-facing. This is not a dashboard
  makeover.
- A screen must explain identity, intent, and primary action before the
  operator reads the first field.
- Overlays must be result-oriented. Opening one pauses the current task and
  closing one always resolves to a value or `null`.
- Navigation must be deterministic. Overlay dismissal outranks focus-screen
  pop, and focus-screen pop outranks root-level exit handling.
- Extraction work must preserve save, validation, and runtime wiring semantics
  unless a slice explicitly changes them.
- Visual hierarchy must do the structural work before badges, helper copy, or
  ornamentation are allowed to compete for attention.

## Program workstreams

1. Interaction infrastructure
   - Overlay stack orchestration, focus trapping, scroll locking, result
     contracts, and shell-level back handling.
2. Page scaffolding and visual grammar
   - ScreenScaffold adoption, section surfaces, metadata rows, type hierarchy,
     and action zones.
3. Form and editor ergonomics
   - InputWithPicker, credential composers, staged flows, dirty-state safety,
     and clearer editor segmentation.
4. Operational inspection surfaces
   - log viewers, artifact viewers, worker controls, process outcome panels,
     and destructive confirms.
5. Consistency, accessibility, and validation
   - keyboard support, screen-reader behavior, automated tests, manual QA, and
     final sweep work.

## Global interaction contracts

Every screen and overlay touched by this program must honor the following
contracts.

### Navigation and back handling

- `Back` closes the top-most overlay before it pops a focus screen.
- `Escape` mirrors the same precedence whenever the current surface is
  dismissible.
- Root-level exit confirmation, if present, must only appear after overlays and
  focus screens have been exhausted.
- Unsaved-changes guards must resolve before navigation continues.

### Overlay result semantics

- `openOverlay(Component, props)` returns a `Promise<T | null>`.
- Overlay components must not directly mutate parent draft state through hidden
  shared state.
- `null` means user-driven cancel or dismissal.
- Non-null values mean an explicit result contract was satisfied.
- Multi-step overlays must still resolve through one final typed result.

### Screen structure

- Every focus screen must have a stable page header.
- Every major functional slice must live inside a clear section container.
- Nested editors, rows, or cards must read as children of a section rather
  than peer panels.
- Support content must never outrank the current task.

### Feedback states

- Every dense screen must define loading, empty, error, and stale-data
  presentation.
- Long-running actions must expose pending state without freezing unrelated
  controls.
- Destructive actions must require explicit confirmation.
- Large logs and large lists must avoid unbounded inline expansion.

## Global infrastructure and shared components

Status in this section should only move when code and validation have both
landed.

- [x] Implement `OverlayManager` / `OverlayProvider` with `openOverlay` API.
  - Files: `apps/desktop/ui/src/components/OverlayManager.tsx`
  - Acceptance: `openOverlay(Component, props)` resolves with a typed result or
    `null`, overlays stack predictably, and caller code can await results
    sequentially.
- [x] Implement `FullScreenModal` base component.
  - Files: `apps/desktop/ui/src/components/FullScreenModal.tsx`
  - Acceptance: title bar, close action, focus trap, body scroll lock,
    dismissible contract, and focus restoration on close.
- [x] Implement `ScreenScaffold` for consistent focus-screen layout.
  - Files: `apps/desktop/ui/src/components/ScreenScaffold.tsx`
  - Acceptance: consistent title, description, action cluster, optional summary
    row, and stable content rhythm.
- [x] Implement `InputWithPicker` helper and `FullScreenFileBrowser` fallback.
  - Files: `apps/desktop/ui/src/components/InputWithPicker.tsx`,
    `apps/desktop/ui/src/components/FullScreenFileBrowser.tsx`
  - Acceptance: desktop fast path stays available while overlay fallback works
    on narrow or embedded flows.
- [x] Implement `LogViewerModal` for long textual outputs.
  - Files: `apps/desktop/ui/src/components/LogViewerModal.tsx`
  - Acceptance: copy, select, download, wrapped and unwrapped reading modes,
    and enough room for future search or filtering.
- [x] Implement `ConfirmDialog` / `DestructiveConfirmModal`.
  - Files: `apps/desktop/ui/src/components/ConfirmDialog.tsx`
  - Acceptance: one standard destructive confirmation flow with explicit action
    language and keyboard-safe default focus.
- [x] Implement `ArtifactViewer` overlay.
  - Files: `apps/desktop/ui/src/components/ArtifactViewer.tsx`
  - Acceptance: preview metadata, open or download actions, and graceful
    fallback for unsupported previews.
- [x] Implement `CredentialComposerModal`.
  - Files:
    `apps/desktop/ui/src/components/forms/CredentialComposerModal.tsx`
  - Acceptance: returns credential or config payload through overlay result
    rather than hidden local mutation.
- [x] Implement `StepFlow` primitive for staged flows.
  - Files: `apps/desktop/ui/src/components/wizard/StepFlow.tsx`
  - Acceptance: step transitions, validation gating, back or close safety, and
    typed final result.
- [x] Add unit tests for `OverlayManager` and `FullScreenModal`.
  - Files: `apps/desktop/ui/src/components/OverlayManager.test.tsx`,
    `apps/desktop/ui/src/components/FullScreenModal.test.tsx`
  - Acceptance: focus trap, promise resolution, dismissal, and focus
    restoration are covered.
- [x] Add at least one screen-level integration test that opens an overlay and
      asserts end-to-end result flow.
  - Acceptance: real screen code awaits overlay result and updates local state
    without callback-only plumbing.

## Delivery tracks by screen

### 1. Main shell and process feed (`apps/desktop/ui/src/App.tsx`)

Mission:
Keep the accepted main-feed visual baseline, but make shell-level overlay and
back-stack behavior fully deterministic.

Why this matters:
The shell root is where broken modal precedence, inconsistent dismissal, or
Escape and Back ambiguity becomes systemic.

Track checklist:

- [ ] Capture current navigation and overlay entry points in `App.tsx`.
- [x] Add `OverlayProvider` at the application root and ensure the app is
      wrapped once.
- [x] Centralize shell-level back handling so overlay dismissal outranks
      focus-screen pop.
- [x] Replace or wrap `WorkerStatusIndicator` interactions with managed
      bottom-sheet or popover overlays.
- [ ] Migrate global ad-hoc popovers that leak outside overlay governance.
- [x] Define root-level exit behavior after overlay and focus-screen precedence
      is exhausted.
- [x] Add one integration path proving that pressing Back closes overlays
      before leaving the current screen.

Acceptance snapshot:

- The main feed still looks like the accepted baseline.
- Overlay interactions no longer bypass the central stack.
- The shell behaves consistently whether the operator presses close, Escape, or
  Back.

### 2. Projects list (`apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`)

Mission:
Turn the projects list into the clearest proof that the new focus-screen
grammar improves scanning without lowering density.

Track checklist:

- [x] Extract `ProjectCard` and `ProjectList` components.
- [x] Add `ScreenScaffold` wrapper and unify header actions.
- [x] Implement `SelectListFullScreen` for large searches via `openOverlay`.
- [x] Integrate `InputWithPicker` for any path or picker fields within project
      list interactions.
- [x] Introduce `ProjectQuickView` as a managed quick-action overlay where fast
      inspection helps.
- [x] Normalize empty, loading, refresh, and error states inside the list
      section.
- [x] Reduce badge noise so repository identity and primary state remain the
      first scan anchors.
- [x] Add an integration test proving list interaction still works when a
      picker or selection overlay returns a value.
- [x] Run visual QA and accessibility checks.

Acceptance snapshot:

- Repository name is the dominant anchor.
- Secondary facts live in quieter metadata rows.
- Refresh, quick actions, and selection flows feel like part of one page rather
  than separate widgets.

### 3. Project detail / repository editor (`apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`)

Mission:
Make the largest editing surface readable, decomposed, and overlay-capable
without changing the underlying save contract.

Track checklist:

- [x] Break the large form into `FormSection` components and
      `BuildTargetEditor` subcomponents.
- [x] Replace `PathPickerField` triggers with `InputWithPicker` that can call
      `openOverlay(FullScreenFileBrowser)`.
- [x] Replace accordion-based top-level editing with icon-tab sections for
      project settings, repository, paths, build targets, publish
      destinations, and runtime status.
- [x] Block project editing while related build or publish processes are
      running and show a clear operator warning.
- [x] Move credential composers to `CredentialComposerModal` overlays.
- [ ] Extract stable summary rows for collapsed build-target and publish-
      destination sections.
- [x] Preserve dirty-state detection, save, reload, and validation semantics
      after extraction.
- [ ] Separate support content, inline warnings, and actionable editors so the
      operator can see ownership immediately.
- [x] Run targeted tests and verify saving behavior is unchanged.
  - Current status: focused `RepositoryProjectDetail` coverage now also locks
    dirty-state detection, successful save/reload round-trips, and manual
    reload discarding unsaved draft edits without firing a save call.
- [ ] Add manual QA for keyboard traversal through long forms and nested
      accordions.

Acceptance snapshot:

- The page announces project identity and action ownership before the first
  section.
- Section headers remain informative while collapsed.
- Nested target editors are obviously children of their parent sections, not
  competing peers.

### 4. Create project wizard (`apps/desktop/ui/src/components/CreateProjectWizard.tsx`)

Mission:
Turn the wizard into a staged transaction with explicit step ownership, not a
loose collection of form panels.

Track checklist:

- [x] Convert the wizard to a `StepFlow` primitive so each step can be a pushed
      screen or an overlay.
- [ ] Provide `onResult` wiring from auth and credential overlays.
  - Current status: credential overlay return now flows through the shared
    publish-destinations editor inside the wizard. Auth overlay result wiring
    is still pending.
- [ ] Separate wizard-level frame, step content, and support content into
      distinct hierarchy levels.
- [ ] Add explicit cancel, close, and unsaved-progress contracts.
- [ ] Make the review step the strongest confirmation surface in the flow.
- [ ] Ensure step-level validation failures are local, legible, and do not
      corrupt adjacent steps.
- [ ] Add integration coverage for cancel, resume, confirm, and overlay-return
      behavior.

Acceptance snapshot:

- The wizard reads as one coherent flow.
- Support callouts no longer compete with the current step.
- Review is a final checkpoint, not just another accordion of facts.

### 5. Project workers (`apps/desktop/ui/src/components/ProjectWorkersFocusScreen.tsx`)

Mission:
Preserve operational density while separating runtime-wide state from per-
project or per-worker state.

Track checklist:

- [x] Extract `RuntimeToolbar` and `ProjectWorkerAccordion` components.
- [x] Implement bulk `SelectListFullScreen` for mass actions.
- [x] Route destructive toolbar actions through overlay-based confirmations.
- [x] Distinguish runtime summary, project grouping, and worker item hierarchy
      visually.
- [x] Normalize loading, empty, unavailable, and stale-data states for worker
      inventories.
- [x] Add focused validation for destructive-action cancel paths, retryable
      inventory failures, and successful bulk actions.

Current status:

- Runtime-wide controls now live in a dedicated `Runtime Controls` panel above
  the worker inventory instead of competing with the page header actions or
  the project-group accordions.
- Runtime `Stop` and `Restart` now route through confirmation overlays before
  mutating shell state.
- Worker inventory now supports bulk instant checks through the shared
  `SelectListFullScreen` flow plus an explicit batch confirmation step.
- Worker inventory refresh now distinguishes loading, unavailable, empty, and
  stale snapshot states while preserving the last known inventory when a
  refresh fails.
- Focused coverage now locks the hierarchy between the runtime controls panel
  and the worker inventory accordion, destructive-action cancel paths, retry
  affordances for failed worker inspection, and bulk instant-check queueing.
- Next resume point: continue the cross-screen feedback-state sweep outside
  `ProjectWorkersFocusScreen`, then retry native Tauri QA from an
  operator-visible desktop session.

Acceptance snapshot:

- Runtime summary and worker items do not compete for the same visual rank.
- Mass actions are deliberate and reversible where possible.
- Dense worker inventories remain scannable under load.

### 6. Process detail (`apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx`)

Mission:
Make process inspection workable for large outputs by moving heavy reading
tasks into purpose-built viewers.

Track checklist:

- [ ] Extract `ExecutionReportPanel`, `OutputsPanel`, and `RetainedLogsPanel`
      into smaller modules.
- [x] Implement `LogViewerModal` and open it with
      `openOverlay(LogViewerModal, { content })` for large logs.
- [x] Implement `ArtifactViewer` overlay for artifacts and retained outputs.
- [ ] Add copy, select, and download actions for log content.
  - Current status: `LogViewerModal` already provides copy-to-clipboard and a
    full-screen readable surface for log selection. A dedicated download
    action is still pending.
- [x] Route destructive retention or cleanup actions through confirm overlays.
- [x] Keep compact outcome summaries inline while moving heavy payloads into
      overlays.
- [x] Add validation for large-log interaction, overlay dismissal, and artifact
      preview fallbacks.
  - Current status: focused `ProcessDetailFocusScreen` coverage now proves
    retained-report viewer opening, retained-log loading into the viewer,
    destructive confirm flows, artifact viewer host-action forwarding, viewer
    dismissal through `Escape`, and close-button dismissal with focus restored
    to the invoking control.

Acceptance snapshot:

- Operators can inspect long logs without bloating the page.
- Output-related actions stay close to the output they affect.
- The screen remains useful even when a process has many retained artifacts and
  logs.

### 7. Auth providers (`apps/desktop/ui/src/components/AuthProvidersFocusScreen.tsx`)

Mission:
Make provider binding feel like a guided credential flow rather than a set of
opaque cards and buttons.

Track checklist:

- [x] Open multi-step auth flows in `OAuthModal` overlays or `StepFlow`-backed
      overlays.
  - Current status: the guided auth connection flow now opens in a
    `StepFlow`-backed full-screen overlay and resolves the refreshed provider
    state back into the focus screen.
- [x] Return token or credential payloads via `openOverlay` result contracts.
  - Current status: host-backed GitHub auth now resolves a semantic
    `AuthProviderConnectionResult` through `openOverlay`, including the
    refreshed provider state, session event label, and operator-facing
    message. Raw token or secret payloads remain unavailable by design because
    Git Credential Manager owns the credential.
- [x] Standardize provider identity, bound state, last-sync data, and actions
      into one consistent card grammar.
  - Current status: provider inventory cards now expose provider identity,
    connection state, reusable credential naming, bound project counts,
    persisted credential stored or refreshed timestamps, session lifecycle
    labels, next-step guidance, and a context-aware review action.
- [x] Handle retry, cancel, expired token, and rebind flows with explicit
      operator feedback.
- [x] Add integration coverage for successful bind, dismissal, and recovery
      from a failed auth attempt.

Acceptance snapshot:

- Binding and rebinding are legible, sequential, and cancel-safe.
- Provider cards communicate state without badge spam.
- Credential acquisition behaves like a first-class flow, not a side effect.

### 8. Publish destinations editor (`apps/desktop/ui/src/components/PublishDestinationsEditor.tsx`)

Mission:
Separate destination identity, target binding, and credential composition into
explicit subflows that can scale with complexity.

Track checklist:

- [x] Convert `BindingSelector` to `SelectListFullScreen` when target
      inventories are large.
- [x] Move credential composition into `CredentialComposerModal` overlays.
- [x] Split destination identity, binding rules, and credential state into
      distinct form surfaces.
- [ ] Preserve existing save behavior and validation messages unchanged.
- [x] Add overlay-result coverage for credential return and binding selection.
  - Current status: focused interactions now cover credential overlay return
    and the large-inventory binding-selection overlay path.
- [ ] Ensure the editor remains usable when a project has many build targets
      and many destinations.

Acceptance snapshot:

- Saving a destination behaves exactly as before.
- Complex binding choices no longer force cramped inline selectors.
- Credential configuration becomes explicit and reviewable.

## Cross-cutting quality tracks

### Feedback-state completeness

- [ ] Audit every touched screen for loading, empty, error, stale, and
      partially loaded states.
- [ ] Standardize retry language and placement.
- [ ] Ensure overlays surface pending work without freezing unrelated content.

Current status:

- `ProjectWorkersFocusScreen` now distinguishes loading, unavailable, empty,
  and stale worker-inventory states with an explicit retry affordance and
  stale snapshot preservation.
- The next sweep should start with any remaining touched focus screens that
  still collapse unavailable data into a loading state, then align retry copy
  and placement across those screens.

### Accessibility and keyboard support

- [ ] Verify initial focus placement for every full-screen overlay.
- [ ] Verify focus restoration to the invoking control.
- [ ] Verify keyboard escape routes, tab order, and button labeling.
- [ ] Verify screen-reader naming for page headers, modal titles, and
      destructive confirmations.

Current status:

- `FullScreenModal` already covers focus trapping and overlay Escape handling.
- `WorkerStatusQuickView` now has dedicated focus-placement coverage for
  loading, empty, and populated states, including the previously broken
  loading or empty autofocus branch.
- `ProjectQuickView` integration coverage now asserts autofocus on its primary
  action when the overlay opens.

### Visual and motion consistency

- [ ] Normalize page-header action placement across focus screens.
- [ ] Normalize metadata rows, chip tones, and summary strip usage.
- [ ] Ensure motion timings and entrance or exit semantics do not conflict
      between overlays and page transitions.

Current status:

- `ProjectsFocusScreen` now routes its eyebrow, description, and inventory
  summary through `ScreenScaffold`, removing one of the remaining page-header
  grammar mismatches between the project list and the other focus screens.
- The process-detail execution-report panel now wraps its header actions onto a
  dedicated row so dense action clusters stop crushing the section copy at the
  default desktop shell width.

### Testing and validation

- [x] Add or update unit tests for extracted presentational primitives where
      logic is non-trivial.
- [x] Add at least one integration test per major overlay family: picker,
      confirm, viewer, and staged flow.
- [x] Run `npm run build --prefix apps/desktop/ui` after each delivery package.
- [x] Maintain a short manual QA checklist covering keyboard, contrast,
      density, and overlay dismissal.

Current status:

- Focused overlay regression coverage now includes shell overlay precedence,
  worker quick-view autofocus states, project quick-view autofocus, confirm
  dialog resolution, artifact-viewer host-action autofocus fallback, log-viewer
  copy semantics, path-picker native-or-overlay fallback, input-with-picker
  overlay result forwarding, modal focus trapping, project-workers runtime
  hierarchy separation, repository-project save/reload draft preservation, and
  Playwright shell flows for worker overlay dismissal, picker Escape dismissal,
  and back-to-main navigation.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest overlay
  accessibility slice and is passing.
- The native Tauri dev shell now starts cleanly via `npm start`; the remaining
  validation gap is live visual parity in the real window, not missing
  overlay-family coverage in the React harness.
- A native-shell inspection attempt in this session confirmed that the HGP
  process launches, but this desktop environment only exposes a 14x14 visible
  window stub while the larger Tauri frames remain hidden and render black via
  both screen capture and `PrintWindow`; visual parity in the real shell still
  requires an operator-facing interactive desktop session.

Short manual QA checklist:

- Keyboard: verify the first actionable control receives focus in each overlay,
  `Escape` dismisses the top-most overlay, and Back returns to the previous
  screen only when no overlay is open.
- Contrast: inspect error banners, disabled controls, muted badges, and focus
  outlines against the dark shell surfaces.
- Density: confirm the main feed, action bars, focus-screen headers, and modal
  toolbars stay compact without clipping at the default desktop shell size.
- Overlay dismissal: verify close button, cancel action, and successful resolve
  paths restore focus to the invoking control when the control still exists.

## Recommended sequencing

1. Harden shared overlay, scaffold, and test infrastructure.
2. Fix shell-level navigation and overlay precedence in `App.tsx`.
3. Finish high-ROI form flows: project detail, path pickers, and publish
   credential flows.
4. Upgrade heavy inspection surfaces: process detail, logs, artifacts, and
   worker controls.
5. Convert staged or multi-step flows: create project wizard and auth
   providers.
6. Run final consistency, accessibility, and validation sweeps.

## Immediate implementation checklist

The next execution slice should start with credential-composition overlays and
staged-flow foundations. Shell-level overlay governance already landed far
enough to unblock the form-heavy screens, while repository and publish flows
still depend on inline credential composition and the wizard or auth surfaces
still lack a reusable staged transaction primitive.

### Slice 01 - Shell overlay governance in `App.tsx`

Objective:
Make shell dismissal deterministic without visually redesigning the main feed.

Expected outcome:

- `Escape` and shell-level back handling dismiss the top-most overlay before
  they pop a focus screen
- the worker status quick surface no longer relies on ad-hoc local tooltip
  state as the only interaction path
- the main feed keeps the same visual baseline while navigation semantics stop
  drifting by screen

#### File checklist

`apps/desktop/ui/src/components/OverlayManager.tsx`

- [x] Extend the overlay context so shell consumers can inspect whether an
      overlay is open.
- [x] Add a shell-safe dismiss API for the top-most overlay.
- [x] Preserve the existing `openOverlay(Component, props)` promise contract.
- [x] Ensure dismissing the top-most overlay still restores focus correctly.
- [x] Avoid introducing overlay-specific business logic into the manager.

`apps/desktop/ui/src/components/OverlayManager.test.tsx`

- [x] Add coverage proving the manager can dismiss the top-most overlay through
      the new public API.
- [x] Add coverage proving stacked overlays dismiss in last-in-first-out order.
- [x] Keep the existing promise-resolution and focus-restoration assertions
      passing.

`apps/desktop/ui/src/App.tsx`

- [x] Audit all shell-level overlay entry points and local popover state.
- [x] Route shell-level `Escape` handling through overlay precedence first.
- [x] Route the focus-screen back action through overlay precedence before
      calling `handleReturnFromFocus`.
- [x] Preserve current window transition behavior and accepted main-feed
      visuals.
- [x] Remove or reduce local shell tooltip state that duplicates overlay stack
      responsibility.
- [x] Keep runtime actions, feed loading, and screen transitions behaviorally
      unchanged.

`apps/desktop/ui/src/components/WorkerStatusIndicator.tsx`

- [x] Verify the indicator remains a dumb trigger component.
- [x] Add only the trigger semantics needed for the governed quick surface.
- [x] Do not move worker inventory or runtime logic into this component.

Optional extraction if `App.tsx` remains too fat after the first pass:

- [x] Extract the current worker quick-view content into a dedicated shell
      overlay or popover component under `apps/desktop/ui/src/components`.
- [x] Keep the extracted component presentational and driven by props from
      `App.tsx`.

#### Validation checklist

Automated:

- [x] Run `npm run build --prefix apps/desktop/ui`.
- [x] Run the overlay manager unit tests.
- [x] Add or update one focused shell interaction test if the current harness
      supports it cleanly.

Manual QA:

- [ ] Open a picker overlay from a focus screen and confirm `Escape` closes the
      overlay before any screen navigation occurs.
- [ ] Open the worker quick surface and confirm dismiss behavior is consistent
      with other overlays.
- [ ] From a focus screen, click the back action while no overlay is open and
      confirm the screen returns normally.
- [ ] Confirm the main feed layout and action bars remain visually unchanged.

Automated proxy coverage now proves overlay-first `Escape` dismissal, overlay
focus restoration, Back without an open overlay, worker quick-view
autofocus in the React harness, and shell-level picker or back flows in
Playwright. A live Tauri pass is still required for the manual checklist above
if native window parity must be observed. The current automation environment
can launch the app, but it does not expose a capturable interactive shell
surface for trustworthy visual verification.

#### Stop conditions for this slice

Current status:

- Overlay precedence now exists for app-level `Escape` dismissal and the
  focus-screen back button.
- Shell regression coverage now also locks Back-without-overlay navigation and
  the worker-status trigger as a dumb, accessibility-safe shell control.
- A real native or host-level back request hook is still pending if the shell
  must react to anything beyond the current in-app controls.

- Stop if the shell lacks a real hook for native back requests and document the
  missing integration point.
- Stop if overlay precedence requires changing the focus-screen routing model
  itself rather than the shell boundary.
- Stop if the worker quick surface needs a broader design decision than a thin
  governed overlay wrapper.

### Slice 02 - Follow-up after shell governance lands

Current status:

- `LogViewerModal` is already landed for process detail.
- At least one representative screen-level overlay-return integration path is
  already covered.
- `CredentialComposerModal` is now landed and wired into publish credential
  flows used by project detail and project creation.
- `StepFlow` is now landed and powers `CreateProjectWizard.tsx`.
- Auth providers now run their guided connection flow inside a
  `StepFlow`-backed overlay and resolve refreshed provider state back into the
  focus screen.
- Auth overlays now return a semantic auth outcome contract instead of a raw
  provider payload, and provider cards surface session lifecycle guidance from
  that result.
- Persisted credential created or updated timestamps now flow from the shell to
  the auth provider cards, closing the current UI-side auth lifecycle gap.

Next targets:

- runtime-side auth work only if new provider lifecycle signals beyond
  credential timestamps become available

## Definition of done

The initiative is only done when:

- focus screens share one predictable navigation model
- overlays use `openOverlay(Component, props)` and resolve through typed
  results
- page, section, and item hierarchy is legible without badge overload
- long logs, large pickers, and credential flows no longer rely on cramped
  inline UI
- save, validation, and runtime wiring semantics remain intact
- accessibility behavior has been manually verified on the touched surfaces
- the desktop UI build and the relevant automated tests are passing
