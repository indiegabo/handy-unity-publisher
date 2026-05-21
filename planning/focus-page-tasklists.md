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

- [x] Capture current navigation and overlay entry points in `App.tsx`.
  - Current status: the shell action bars now enumerate their navigation entry
    points through explicit action inventories, the focus-shell presentation is
    resolved through one shared helper instead of nested ternaries, and the
    file documents the overlay entry points it still governs locally.
- [x] Add `OverlayProvider` at the application root and ensure the app is
      wrapped once.
- [x] Centralize shell-level back handling so overlay dismissal outranks
      focus-screen pop.
- [x] Replace or wrap `WorkerStatusIndicator` interactions with managed
      bottom-sheet or popover overlays.
- [x] Migrate global ad-hoc popovers that leak outside overlay governance.
  - Current status: the worker-status trigger now exposes its longer runtime
    summary through accessible description text instead of a native `title`
    tooltip, leaving the governed quick-view overlay as the only rich worker
    inspection surface in the shell.
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
- [x] Extract stable summary rows for collapsed build-target and publish-
      destination sections.
  - Current status: collapsed build targets now show a stable execution
    summary for build method, Unity executable readiness, and bound publish
    destinations before the editor is reopened, while publish destinations
    keep their binding and credential summaries in the accordion header.
- [x] Preserve dirty-state detection, save, reload, and validation semantics
      after extraction.
- [x] Separate support content, inline warnings, and actionable editors so the
      operator can see ownership immediately.
  - Current status: the destinations tab now renders `Draft impact` as a
    dedicated support panel beside the actionable editor instead of a generic
    inline wizard callout, so draft guidance no longer competes with the edit
    controls.
- [x] Run targeted tests and verify saving behavior is unchanged.
  - Current status: focused `RepositoryProjectDetail` coverage now also locks
    dirty-state detection, successful save/reload round-trips, and manual
    reload discarding unsaved draft edits without firing a save call.
- [x] Add automated regression coverage for keyboard traversal through long
      forms and nested accordion-backed editors.
  - Current status: focused `RepositoryProjectDetail` coverage now locks the
    focus order through the publish destination accordion, credential
    controls, and bound-target editor controls.

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
- [x] Provide `onResult` wiring from auth and credential overlays.
  - Current status: credential overlay return now flows through the shared
    publish-destinations editor inside the wizard. GitHub login result wiring
    is now covered inside the staged flow, and the broader auth-provider
    management round-trip now returns through `App.tsx` with a typed auth
    result that the repository-access step consumes back into the draft.
- [x] Separate wizard-level frame, step content, and support content into
      distinct hierarchy levels.
- [x] Add explicit cancel, close, and unsaved-progress contracts.
- [x] Make the review step the strongest confirmation surface in the flow.
- [x] Ensure step-level validation failures are local, legible, and do not
      corrupt adjacent steps.
  - Current status: repository inventory failures now block only the affected
    identity or access steps, while colocated retry actions recover inventory,
    accounts, credentials, and access checks without forcing a full wizard
    restart. Focused wizard coverage now also locks late-step path validation
    to the `Paths` step so invalid overrides stop advancement without leaking
    the review surface into the failure state.
- [x] Add integration coverage for cancel, resume, confirm, and overlay-return
      behavior.
  - Current status: focused wizard and app coverage now locks the retryable
    inventory failure path, explicit cancel delegation, GitHub login result
    wiring, final-review confirmation gating, unsaved draft discard
    confirmation, draft resume from saved snapshots, review reconfirmation
    after leaving the final step, and draft resume after returning from auth
    providers.

Current status:

- `BuildTargetRemovalCallout` now centralizes the destructive build-target
  removal copy shared by `CreateProjectWizard` and
  `RepositoryProjectDetail`, so build-target removal no longer diverges
  between the staged creation flow and the project editor.
- Wizard drafts now emit a serializable snapshot plus dirty-state updates into
  `App.tsx`, so the flow can resume after auth-provider detours and require an
  explicit discard decision before closing or backing out.
- Auth-provider management now returns a typed result through `App.tsx`, so a
  successful GitHub reconnect can be re-applied to the repository-access step
  instead of relying on a blind remount to rediscover state.
- The wizard now routes identity guidance, repository-access controls, and the
  final review confirmation through a dedicated support rail beside the active
  step panel instead of mixing those concerns into the form body.
- The review step now requires an explicit final confirmation checkbox before
  project creation can proceed.

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
- Feedback-state cleanup across the remaining touched focus screens is now
  complete.
- Next resume point: run the live native Tauri QA checklist from an
  operator-visible desktop session and feed any visual-parity findings back
  into the remaining publish and long-form slices.

Acceptance snapshot:

- Runtime summary and worker items do not compete for the same visual rank.
- Mass actions are deliberate and reversible where possible.
- Dense worker inventories remain scannable under load.

### 6. Process detail (`apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx`)

Mission:
Make process inspection workable for large outputs by moving heavy reading
tasks into purpose-built viewers.

Track checklist:

- [x] Extract `ExecutionReportPanel`, `OutputsPanel`, and `RetainedLogsPanel`
      into smaller modules.
- [x] Implement `LogViewerModal` and open it with
      `openOverlay(LogViewerModal, { content })` for large logs.
- [x] Implement `ArtifactViewer` overlay for artifacts and retained outputs.
- [x] Add copy, select, and download actions for log content.
  - Current status: `LogViewerModal` now provides copy-to-clipboard, a
    full-screen readable surface for log selection, and a dedicated download
    action with stable file names for retained reports and retained logs.
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

Current status:

- `RetainedLogsPanel`, `ExecutionReportPanel`, and `OutputsPanel` are now
  extracted from `ProcessDetailFocusScreen`, leaving the owner focused on
  orchestration, overlays, and host-action wiring instead of dense section
  markup.
- Retained report and retained log viewers now expose a direct download action
  alongside copy and wrap controls, with the process-detail call sites passing
  stable file names into the shared viewer.

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
- [x] Preserve existing save behavior and validation messages unchanged.
  - Current status: focused `RepositoryProjectDetail` coverage now locks a
    publish-destination save round-trip plus validation-error rendering and
    save blocking inside the destinations section.
- [x] Add overlay-result coverage for credential return and binding selection.
  - Current status: focused interactions now cover credential overlay return
    and the large-inventory binding-selection overlay path.
- [x] Ensure the editor remains usable when a project has many build targets
      and many destinations.
  - Current status: collapsed destination headers now expose binding counts,
    target previews, and credential or delivery summaries so dense publish
    inventories stay scannable without reopening every accordion.

Acceptance snapshot:

- Saving a destination behaves exactly as before.
- Complex binding choices no longer force cramped inline selectors.
- Credential configuration becomes explicit and reviewable.

## Cross-cutting quality tracks

### Feedback-state completeness

- [x] Audit every touched screen for loading, empty, error, stale, and
      partially loaded states.
- [x] Standardize retry language and placement.
- [x] Ensure overlays surface pending work without freezing unrelated content.

Current status:

- The feedback-state sweep is now complete across `ProjectsFocusScreen`,
  `ProjectWorkersFocusScreen`, `ProcessDetailFocusScreen`,
  `AuthProvidersFocusScreen`, `RepositoryProjectDetail`, and
  `CreateProjectWizard`.
- Retry actions now sit beside the failed resource instead of collapsing into
  passive banners, and last-known-good snapshots are preserved where that
  keeps the UI truthful during refresh failures.
- The remaining gap for this track is native-window observation, not missing
  feedback-state contracts in the React harness.

### Accessibility and keyboard support

- [x] Verify initial focus placement for every full-screen overlay.
- [x] Verify focus restoration to the invoking control.
- [x] Verify keyboard escape routes, tab order, and button labeling.
- [x] Verify screen-reader naming for page headers, modal titles, and
      destructive confirmations.

Current status:

- `FullScreenModal` already covers focus trapping and overlay Escape handling,
  and now also ignores `data-overlay-autofocus="false"` while honoring
  preferred focus targets that intentionally use `tabIndex={-1}`.
- `WorkerStatusQuickView` now has dedicated focus-placement coverage for
  loading, empty, and populated states, including the previously broken
  loading or empty autofocus branch, and `App` integration coverage now locks
  focus restoration back to the worker-status trigger when the quick view is
  dismissed through `Escape` or the modal close button.
- `ProcessDetailFocusScreen` now covers retained log viewer dismissal through
  the modal close button and restores focus to the invoking `Open viewer`
  control, alongside the existing retained report and artifact viewer overlay
  restoration paths.
- `ProjectQuickView` integration coverage now asserts autofocus on its primary
  action when the overlay opens and focus restoration back to the quick-view
  trigger when the overlay is dismissed with `Escape` or the modal close
  button.
- `PathPickerField` now asserts autofocus and focus restoration for the
  fallback manual path overlay when it is dismissed through `Escape` or the
  modal close button, and `FullScreenFileBrowser` explicitly marks the path
  input as the preferred focus target instead of the modal close button.
- `PublishDestinationsEditor` interactions now assert autofocus and focus
  restoration for both the large-inventory target selector and the publish
  credential composer when they are dismissed through `Escape`, `Cancel`, or
  the modal close button, and the composer now prefers the credential-name
  field over the modal close button on entry.
- `AuthProvidersFocusScreen` now asserts autofocus on the auth overlay primary
  action and focus restoration back to the invoking review trigger after
  `Escape` and the close button, and `AuthProviderConnectionModal` explicitly
  marks its primary action as the preferred focus target.
- `App` shell actions and `ProcessFeedItem` now use English screen-reader
  labels for icon-only controls such as project navigation, window controls,
  back navigation, and opening process detail, with integration coverage
  locked in `App.test.tsx`.
- `InputWithPicker` now explicitly wires its visible label, hint, and error
  text to the underlying textbox accessibility tree, with regression coverage
  in `InputWithPicker.test.tsx` so quick-open and picker-backed fields expose
  a stable accessible name.
- `RepositoryProjectDetail` now supports `ArrowUp`, `ArrowDown`, `Home`, and
  `End` navigation across its vertical section tablist, with focus and
  selection transitions locked in `RepositoryProjectDetail.test.tsx`.
- `ProjectsFocusScreen` quick open now supports `ArrowDown` to move from the
  filter input to the first filtered project card and `ArrowUp` to jump to
  the last one, while `Enter` opens an exact repository-name match; once focus
  is on a project card, `ArrowUp`, `ArrowDown`, `Home`, and `End` now move
  across the filtered card list, with owner coverage locked in
  `ProjectsFocusScreen.test.tsx`.
- `VerticalAccordion` now has direct keyboard regression coverage proving that
  header-interactive accordions toggle from `Enter` and `Space`, so shared
  dense editor surfaces do not quietly regress into click-only behavior.
- `ProcessDetailFocusScreen` now uses consistent destructive naming for the
  outputs and retained-material delete confirmations so the trigger text,
  dialog title, and confirm action describe the same operation.
- `ProjectCard` now exposes repository-specific accessible names for both the
  primary open action and the quick-view trigger, keeping the project list
  scannable in the accessibility tree even when multiple cards share similar
  visual structure.
- `SelectListFullScreen` now has direct owner coverage for filter autofocus,
  `Escape` dismissal focus restoration, close-button focus restoration, and
  Arrow-key or Home or End traversal between the filter field and result
  buttons, while `ConfirmDialog` now explicitly locks its destructive dialog
  title and action labels through the accessibility tree.
- Build-target removal confirmation copy is now aligned between
  `CreateProjectWizard` and `RepositoryProjectDetail`, so the destructive
  action consistently states that both the build target and its publish
  bindings will be removed.

### Visual and motion consistency

- [x] Normalize page-header action placement across focus screens.
  - Current status: `AuthProvidersFocusScreen` now routes its page identity,
    summary strip, and refresh action through `ScreenScaffold`, bringing the
    remaining standard account-management focus screen onto the same header
    contract already used by projects, workers, and process detail.
- [x] Normalize metadata rows, chip tones, and summary strip usage.
- [x] Ensure motion timings and entrance or exit semantics do not conflict
      between overlays and page transitions.

Current status:

- `ProjectsFocusScreen` now routes its eyebrow, description, and inventory
  summary through `ScreenScaffold`, removing one of the remaining page-header
  grammar mismatches between the project list and the other focus screens.
- `ProjectWorkersFocusScreen` and `ProcessDetailFocusScreen` now also route
  their page identity, summary, and action grammar through `ScreenScaffold`,
  shrinking another pair of inconsistent focus-screen headers down to the same
  layout contract.
- `AuthProvidersFocusScreen` now uses `ScreenScaffold` for its page header,
  so the login-management surface no longer keeps a one-off header wrapper for
  its refresh action and inventory summary.
- The auth-provider inventory cards and the guided connection overlay now both
  route provider lifecycle metadata through shared summary strips, so the
  operator sees the same compact inventory grammar whether they stay on the
  screen or drill into the browser-backed reconnect flow.
- The process-detail artifact inspection surfaces now route their top metadata
  through shared summary-strip grammar, and publish-target kind chips are now
  consistently demoted behind status chips across `ArtifactViewer` and
  `OutputsPanel`.
- The project list cards and the project quick-view overlay now also route
  repository metadata through shared summary strips, removing another pair of
  ad hoc inventory wrappers from the browse-and-inspect path.
- `SelectListFullScreen` now routes its inventory totals through a shared
  summary strip, and the manual-path fallback overlay no longer borrows the
  project-card summary style for unrelated picker guidance.
- `ExecutionReportPanel` and `RetainedLogsPanel` now route their top metadata
  through `SurfacePanel` summaries instead of rendering ad hoc inventory rows
  inside the body, so the remaining process-detail support panels speak the
  same summary-strip grammar as the artifact and output surfaces.
- `ProcessDetailFocusScreen` now routes both the final-outcome snapshot and the
  runtime metadata inventory through shared panel summaries, so the operator's
  timestamp and process-status context stops competing with the panel body.
- `ProjectWorkersFocusScreen` now uses shared summary strips inside the worker
  inventory and per-project accordions, and `WorkerStatusQuickView` now uses
  the same grammar for runtime and worker totals instead of a one-off paragraph
  grid.
- `ProcessFeedItem` now routes release badges through a shared summary strip in
  the collapsed feed card header, and `LogViewerModal` now routes viewer meta
  and action feedback through the same shared strip grammar instead of loose
  paragraph blocks.
- `PublishDestinationsEditor` and `RepositoryProjectDetail` now route their
  remaining collapsed destination and project-detail support summaries through
  shared strips, closing the last summary-specific outliers in the focus UI
  surfaces outside the still-separate manual QA checklist.
- The process-detail execution-report panel now wraps its header actions onto a
  dedicated row so dense action clusters stop crushing the section copy at the
  default desktop shell width.
- Shell page transitions now respect `prefers-reduced-motion`, while shared
  motion tokens drive the worker indicator, buttons, accordion expansion, and
  selection overlays so page switches stop drifting from the rest of the UI's
  timing contract.
- The remaining raw motion durations in `styles.css` are now centralized behind
  shared tokens, and reduced-motion mode also disables the animated process-feed
  border so shell transitions, feed chrome, and overlay-adjacent controls all
  degrade under one motion contract.

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
- `App` coverage now also locks runtime-event-driven worker status updates and
  repository inspection refreshes, so the shell contract between
  `runtime:event` delivery and the worker-status control is no longer implicit.
- The latest process-detail viewer slice reran focused owner suites for
  `LogViewerModal` and `ProcessDetailFocusScreen`, then reran the full
  desktop-UI automated suite with 116 passing tests.
- The latest shell and auth-header consistency slice reran focused owner
  suites for `App` and `AuthProvidersFocusScreen`, then reran the full
  desktop-UI automated suite with 118 passing tests.
- The latest artifact-summary and motion-consistency slice reran focused owner
  suites for `ArtifactViewer`, `ProcessDetailFocusScreen`, and `App`, then
  reran the full desktop-UI automated suite with 120 passing tests.
- The latest process-detail summary and motion-token cleanup slice reran the
  focused owner suite for `ProcessDetailFocusScreen`, then reran the full
  desktop-UI automated suite with 120 passing tests.
- The latest auth-provider and project-summary normalization slice reran
  focused owner suites for `AuthProvidersFocusScreen` and
  `ProjectsFocusScreen`, then reran the full desktop-UI automated suite with
  122 passing tests.
- The latest non-wizard summary-normalization slice reran focused owner suites
  for `SelectListFullScreen`, `PathPickerField`, `ProcessDetailFocusScreen`,
  `ProjectWorkersFocusScreen`, and `WorkerStatusQuickView`, then reran the
  full desktop-UI automated suite with 126 passing tests.
- The latest process-feed and log-viewer normalization slice reran focused
  owner suites for `ProcessFeedItem` and `LogViewerModal`, then reran the full
  desktop-UI automated suite with 127 passing tests.
- The latest detail-and-publish summary cleanup slice reran focused owner
  suites for `RepositoryProjectDetail` and `PublishDestinationsEditor`, then
  reran the full desktop-UI automated suite with 129 passing tests.
- The latest accessibility automation slice reran focused owner suites for
  `SelectListFullScreen` and `ConfirmDialog`, then reran the full desktop-UI
  automated suite with 133 passing tests.
- The latest runtime-event App integration slice reran the focused owner suite
  for `App`, then reran the full desktop-UI automated suite with 135 passing
  tests.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  artifact-summary and motion-consistency slice and is passing.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  process-detail summary and motion-token cleanup slice and is passing.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  auth-provider and project-summary normalization slice and is passing.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  non-wizard summary-normalization slice and is passing.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  process-feed and log-viewer normalization slice and is passing.
- `npm run build --prefix apps/desktop/ui` was rerun after the latest
  detail-and-publish summary cleanup slice and is passing.
- `cargo build --package desktop-shell` was rerun after the latest shell
  transition changes and is passing.
- `cargo build --package desktop-shell` was rerun after the latest
  process-detail summary and motion-token cleanup slice and is passing.
- `cargo build --package desktop-shell` was rerun after the latest
  auth-provider and project-summary normalization slice and is passing.
- `cargo build --package desktop-shell` was rerun after the latest non-wizard
  summary-normalization slice and is passing.
- `cargo build --package desktop-shell` was rerun after the latest process-feed
  and log-viewer normalization slice and is passing.
- `cargo build --package desktop-shell` was rerun after the latest
  detail-and-publish summary cleanup slice and is passing.
- This slice was closed with automated validation only; live visual parity in
  the native shell still depends on an operator-visible desktop session.
- The native Tauri dev shell now starts cleanly via `npm start`; the remaining
  validation gap is live visual parity in the real window, not missing
  overlay-family coverage in the React harness.
- Repeated native-shell inspection attempts in this session confirmed that the
  HGP process launches, but this desktop environment only exposes a 14x14
  visible window stub while the larger Tauri frames remain hidden and render
  black via both screen capture and `PrintWindow`; visual parity in the real
  shell still requires an operator-facing interactive desktop session.

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

### Native Tauri close-out definition of done

Treat the focus-screen mission as complete only after one operator-visible
desktop session satisfies every check below in the real Tauri shell.

- [x] Verify the main feed layout and action bars match the accepted baseline,
      with no unexpected spacing, wrapping, clipping, or action drift.
  - Current status: verified in an operator-visible Tauri session against the
    accepted main-feed baseline.
- [x] Verify opening the worker quick view from the main feed and dismissing it
      with `Escape` and the close button keeps the operator on the main feed
      and restores focus to the trigger.
  - Current status: verified in an operator-visible Tauri session for both
    `Escape` dismissal and close-button dismissal from the main feed.
- [x] Verify opening a picker or selection overlay from a focus screen and
      pressing `Escape` dismisses only the overlay while leaving the underlying
      focus screen intact.
  - Current status: verified in an operator-visible Tauri session from the
    project-list picker flow, with focus restored to the invoking control.
- [x] Verify Back from a focus screen pops the screen only when no overlay is
      open, and dismisses the overlay first when one is present.
  - Current status: verified in an operator-visible Tauri session from the
    project-list flow with both overlay-first dismissal and normal return to
    the main feed.
- [x] Verify one long-content viewer path (`LogViewerModal` or
      `ArtifactViewer`) remains readable at the default desktop shell size and
      restores focus to the invoking control on close.
  - Current status: verified in an operator-visible Tauri session through a
    process-detail viewer path, including readable content and focus
    restoration on close.
- [x] Verify one staged flow (`CreateProjectWizard` or auth-provider
      reconnect) preserves readable hierarchy, explicit cancel behavior, and a
      safe return to the previous shell context.
  - Current status: verified in an operator-visible Tauri session through the
    create-project wizard, including dirty-draft guard behavior and safe
    return to the previous shell context.
- [x] Verify contrast and density remain acceptable in the real window for
      error banners, disabled controls, muted badges, focus outlines,
      focus-screen headers, and modal toolbars.
  - Current status: verified in an operator-visible Tauri session across the
    main feed, focus screens, and overlay surfaces at the default desktop shell
    size.

Close-out rule:

- When every check above passes in a visible Tauri session, mark the remaining
  shell-level manual item as complete and treat the focus-screen mission as
  done.
- Current status: every close-out check above has now been verified in an
  operator-visible Tauri session, so the focus-screen mission can be treated
  as complete.
- If the automation environment remains blind, record the native-shell blocker
  explicitly instead of reopening completed React or runtime slices.

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

The native Tauri close-out definition above has now been satisfied in an
operator-visible desktop session. The focus-screen mission can be treated as
complete. Any follow-up work from this point should be scoped as a new slice
rather than unresolved close-out debt from the original focus-screen mission.

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

- [x] Cover picker-overlay `Escape` dismissal before any focus-screen
      navigation occurs.
  - Current status: focused owner tests plus Playwright shell coverage now
    lock picker dismissal precedence and focus restoration without relying on
    manual shell observation.
- [x] Cover worker quick-surface dismissal behavior against the shared overlay
      contract.
  - Current status: `App` and `WorkerStatusQuickView` coverage now lock
    autofocus, `Escape`, close-button dismissal, and focus restoration.
- [x] Cover back-action return behavior from focus screens when no overlay is
      open.
  - Current status: `App` integration coverage plus Playwright shell coverage
    now lock normal Back return behavior without an overlay in front.
- [x] Confirm the main feed layout and action bars remain visually unchanged.
  - Current status: verified in an operator-visible Tauri session against the
    accepted main-feed baseline.

Automated proxy coverage now proves overlay-first `Escape` dismissal, overlay
focus restoration, Back without an open overlay, worker quick-view
autofocus in the React harness, runtime-event-driven worker status updates,
and shell-level picker or back flows in Playwright. The main-feed baseline has
now also been verified in an operator-visible Tauri session. Any remaining
native-shell QA belongs to the broader close-out definition of done above, not
to this shell-governance slice. The automation environment can launch the app,
but it does not expose a capturable interactive shell surface for trustworthy
visual verification.

#### Stop conditions for this slice

Current status:

- Overlay precedence now exists for app-level `Escape` dismissal and the
  focus-screen back button.
- Shell regression coverage now also locks Back-without-overlay navigation and
  the worker-status trigger as a dumb, accessibility-safe shell control
  without relying on native browser tooltips.
- `App.tsx` now captures its action-bar navigation entry points and focus-shell
  presentation contract explicitly, so shell routing no longer depends on a
  scattered chain of button literals and nested class-name ternaries.
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
