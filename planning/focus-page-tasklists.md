# Focus-page tasklists — implement mobile-first focus screens with overlay methodology

This document lists detailed tasklists per existing focus page in the HGP desktop UI. Each per-page tasklist is intended to result in the new layout implemented using the project-wide overlay/modal methodology (OverlayManager + FullScreenModal + ScreenScaffold + InputWithPicker patterns).

Guiding constraints

- Deliver UI work in `apps/desktop/ui/src/components` and integrate into the existing focus screens under `apps/desktop/ui/src/components`.
- All runtime behaviour should prefer `openOverlay(Component, props)` patterns returning a Promise-resolved result.
- Keep changes incremental and testable: implement shared infra (OverlayManager, ScreenScaffold, FullScreenModal) before migrating per-page pickers.
- Accessibility: overlays must set `aria-modal`, trap focus and restore focus on close.

Global infra (preconditions)

Note: tasks below are checkable. I will update these checkboxes in this file as I complete each item so you can follow progress.

- [x] Implement `OverlayManager` / `OverlayProvider` with `openOverlay` API.
  - Files: `apps/desktop/ui/src/components/OverlayManager.tsx`
  - Acceptance: `openOverlay(Component, props)` returns a Promise that resolves with overlay result or null on cancel.
- [x] Implement `FullScreenModal` base component.
  - Files: `apps/desktop/ui/src/components/FullScreenModal.tsx`
  - Acceptance: title bar, close action, focus trap, body scroll locked while open.
- [x] Implement `ScreenScaffold` for consistent focus-screen layout.
- [x] Implement `InputWithPicker` helper and `FullScreenFileBrowser` fallback.
- [ ] Implement `LogViewerModal` for large logs.
- [x] Add unit tests for `OverlayManager` and `FullScreenModal`.

Per-focus-page tasklists

For each section below follow the same pattern: analysis → extract/design → implement → integrate → test → polish.

1. Main / Process Feed (`apps/desktop/ui/src/App.tsx`)

- [ ] Capture current navigation patterns and places where ad-hoc overlays are used.
- [x] Add `OverlayProvider` at the application root and ensure `App.tsx` wraps the UI.
- [ ] Replace or wrap `WorkerStatusIndicator` interactions to use `openOverlay(BottomSheet, props)`.
- [ ] Migrate any global popovers to `openOverlay` where appropriate.
- [ ] Acceptance: pressing back closes overlays first; nav stack remains consistent.
- Files to edit: `apps/desktop/ui/src/App.tsx`.

2. Projects list (`apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`)

- [x] Extract `ProjectCard` and `ProjectList` components.
- [x] Add `ScreenScaffold` wrapper and unify header actions.
- [x] Implement `SelectListFullScreen` for large searches (open via `openOverlay`).
- [x] Integrate `InputWithPicker` for any path/picker fields within cards.
- [x] Visual QA and accessibility checks.
- Files to edit: `ProjectsFocusScreen.tsx`, new components under `components/projects/`.

3. Project detail / repository editor (`apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`)

- [x] Break large form into `FormSection` components and `BuildTargetEditor` subcomponents.
- [x] Replace `PathPickerField` triggers with `InputWithPicker` that calls `openOverlay(FullScreenFileBrowser)`.
- [ ] Move credential composers to `CredentialComposerModal` overlays (open via `openOverlay`).
- [ ] Run tests and integrate saving behavior unchanged.
- Files to edit: `RepositoryProjectDetail.tsx`, `PathPickerField.tsx`, `PublishDestinationsEditor.tsx`, new `components/forms/` files.

4. Create Project Wizard (`apps/desktop/ui/src/components/CreateProjectWizard.tsx`)

- [ ] Convert wizard to `StepFlow` primitive; each step can be a pushed screen or an overlay.
- [ ] Provide `onResult` wiring from auth/credential overlays.
- [ ] Add cancel/confirm contracts with clear acceptance criteria.
- Files to edit: `CreateProjectWizard.tsx`, new `components/wizard/` files.

5. Project Workers (`apps/desktop/ui/src/components/ProjectWorkersFocusScreen.tsx`)

- [ ] Extract `RuntimeToolbar` and `ProjectWorkerAccordion` components.
- [ ] Implement bulk `SelectListFullScreen` for mass actions (open via `openOverlay`).
- [ ] Ensure toolbars call overlay-based confirmations for destructive actions.
- Files to edit: `ProjectWorkersFocusScreen.tsx`, new `components/workers/` files.

6. Process Detail (`apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx`)

- [ ] Extract `ExecutionReportPanel`, `OutputsPanel`, `RetainedLogsPanel` into smaller modules.
- [ ] Implement `LogViewerModal` and open with `openOverlay(LogViewer, { content })` for large logs.
- [ ] Implement `ArtifactViewer` overlay to preview artifacts.
- [ ] Acceptance: logs are rendered in modal with copy/select and download actions.
- Files to edit: `ProcessDetailFocusScreen.tsx`, new `components/process/` files.

7. Auth Providers (`apps/desktop/ui/src/components/AuthProvidersFocusScreen.tsx`)

- [ ] For multi-step auth flows, open `OAuthModal` overlays (use `StepFlow` inside modal if needed).
- [ ] Add acceptance path that returns token/credential object via `openOverlay`.
- Files to edit: `AuthProvidersFocusScreen.tsx`, new `components/auth/` files.

8. Publish Destinations Editor (`apps/desktop/ui/src/components/PublishDestinationsEditor.tsx`)

- [ ] Convert `BindingSelector` to `SelectListFullScreen` if targets are large.
- [ ] Move credential composition to `CredentialComposerModal` overlays.
- [ ] Acceptance: saving destination unchanged; modal returns credential/config object.
- Files to edit: `PublishDestinationsEditor.tsx`, `RepositoryCredentialComposer.tsx`.

Cross-cutting tasks per page

- Add automated unit tests for each extracted component where feasible.
- Add at least one integration test that opens a modal via `openOverlay` and asserts result flow.
- Add visual sanity checks: quick manual checklist for spacing, contrast, keyboard access.

Estimates & ordering (recommended)

1. Implement global infra (OverlayManager, FullScreenModal, ScreenScaffold) — required before major migration.
2. Migrate `PathPickerField` to overlay and small pickers (low-risk, high-value).
3. Implement Projects screen layout & Project Detail (high ROI).
4. Implement ProcessDetail and LogViewer (readability win).
5. Implement Workers, Publish Destinations, Auth flows and wizard conversion.
6. Polish, accessibility pass, tests.

Acceptance criteria (global)

- All new overlays use `openOverlay(Component, props)` and return results as Promises.
- Focus trapping and body scroll lock are implemented for all full-screen overlays.
- Visual language remains consistent with desktop UI tokens (colors, radii, spacing).

Next steps

- Implement `OverlayManager` + `FullScreenModal` PoC and minimal unit tests.
- After PoC is green, proceed page-by-page following the per-page tasklists above.
