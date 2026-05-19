# Focus screen mental model and overlays (mobile-first)

Summary

- Objective: Treat focus screens in the shell as smartphone-style screens: push/pop navigable units that may be interrupted by full-screen overlays which return a result.
- Expected outcome: clearer navigation, predictable push/pop flow, overlays with a clear result contract (open -> await result), and reusable components for pickers, confirmations, and long lists.

Mental model

- Screen as unit: each `FocusScreen` (for example, project detail, process detail, create-wizard) should be treated as a full screen unit. Prefer navigation using `push`/`pop` with short transitions.
- Blocking overlays: pickers, critical confirmations and viewers should occupy the full viewport (FullScreen) and return a value when closed. While an overlay exists, the primary flow is paused.
- Non-blocking overlays: bottom sheets and action sheets should keep context visible and allow quick decisions without fully interrupting flow.
- Unified back handling: overlays close first, then the nav stack pops. At the root, optionally surface an exit confirmation.
- Staged forms (wizards): each step is either a pushed screen or a full-screen overlay—use a `StepFlow` helper for sequential validation and navigation.

Design principles

- Visual consistency: use a `ScreenScaffold` (title, actions, content, footer) for all focus screens.
- Result-based APIs: prefer `const result = await openOverlay(Component, props)` to simplify sequential code and reduce callback hell.
- Accessibility: when opening an overlay, move focus to the first interactive element and trap focus; restore focus on close; support `Escape` and `Back` keys.
- Motion: short, clear transitions. Suggested timings: push/pop 200–260ms (ease-out); overlay slide-up 180–220ms; backdrop fade 120–160ms.

Screen-by-screen analysis (findings and recommendations)

- Main / Process Feed — apps/desktop/ui/src/App.tsx
  - Current state: main feed with actions (open project, create project) and explicit transitions to focus screens.
  - Improvements:
    - Normalize `Main` vs `Focus` navigation behind a single `NavStack` API.
    - Replace ad-hoc `WorkerStatusIndicator` tooltip with a managed `BottomSheet` or `Popover` under `OverlayManager` control.
  - Recommended components: `ScreenScaffold`, `NavStack`.

- Project List — apps/desktop/ui/src/components/ProjectsFocusScreen.tsx
  - Current state: grid of project cards with refresh and optional highlight.
  - Improvements:
    - Extract `ProjectCard` and `ProjectList` components for reuse.
    - Add `SelectListFullScreen` for filtering and selecting in large inventories.
    - Provide `ProjectQuickView` as a `BottomSheet` for quick actions like instant check.
  - Recommended components: `ProjectCard`, `SelectListFullScreen`, `BottomSheet`.

- Project Detail / RepositoryProjectDetail — apps/desktop/ui/src/components/RepositoryProjectDetail.tsx
  - Current state: large form split into sections (Project Settings, Build Targets, Publish Destinations, Automation); uses `VerticalAccordion`, `PathPickerField`, `PublishDestinationsEditor`.
  - Improvements:
    - Standardize section UI as `FormSection` / `SectionAccordion` primitives.
    - Provide a `FullScreenFileBrowser` fallback for small viewports while keeping `PathPickerField` as the launcher.
    - Make credential composers (`RepositoryCredentialComposer`, publish credential composer) full-screen overlays where appropriate.
    - Extract `BuildTargetEditor`, `PublishDestinationEditor`, `TargetBindingSelector` into isolated components.
  - Recommended components: `InputWithPicker`, `FullScreenFileBrowser`, `CredentialComposerModal`, `TargetEditor`.

- Create Project Wizard — apps/desktop/ui/src/components/CreateProjectWizard.tsx
  - Current state: wizard embedded in `FocusPageFrame`.
  - Improvements:
    - Encapsulate as `StepFlow` with reusable steps; each step can map to a pushed screen or a full-screen overlay.
    - Allow `AuthProvidersFocusScreen` to be opened as an overlay that returns a credential via `onResult`.
  - Recommended components: `StepFlow`, `WizardStep`, `AuthPickerOverlay`.

- Project Workers — apps/desktop/ui/src/components/ProjectWorkersFocusScreen.tsx
  - Current state: accordion sections for runtime overview and inventory; runtime controls in a toolbar.
  - Improvements:
    - Extract `RuntimeToolbar` and `ProjectWorkerAccordion` primitives.
    - Use `SelectListFullScreen` for bulk selection and actions over many targets.
  - Recommended components: `RuntimeControlToolbar`, `ProjectWorkerAccordion`.

- Process Detail — apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx
  - Current state: panels for final outcome, outputs, retained logs; large log content rendered inline via `VerticalAccordion`.
  - Improvements:
    - Move log reading into a `LogViewerModal` full-screen overlay to improve readability and copy/paste.
    - Add an `ArtifactViewer` overlay and a `ConfirmDialog` overlay for destructive actions.
    - Extract `ExecutionReportPanel`, `OutputsPanel`, `RetainedLogsPanel` to reduce file complexity.
  - Recommended components: `LogViewerModal`, `ArtifactCard`, `ConfirmDialog`.

- Auth Providers — apps/desktop/ui/src/components/AuthProvidersFocusScreen.tsx
  - Current state: list of providers and bind/unbind flows.
  - Improvements:
    - When login flows become multi-step, open the flow as a full-screen `OAuthModal` / `StepFlow` overlay.
  - Recommended components: `ProviderList`, `OAuthModal`.

- Publish Destinations Editor — apps/desktop/ui/src/components/PublishDestinationsEditor.tsx
  - Current state: complex editor with publish bindings and path pickers.
  - Improvements:
    - Make the binding selector a `SelectListFullScreen` when build targets grow.
    - Move credential composition into a modal overlay.
  - Recommended components: `BindingSelector`, `PublishDestinationCard`, `CredentialComposerModal`.

APIs / props (sketches in TypeScript — technical English)

```ts
// Overlay opener that returns a Promise with result when closed
export type OverlayHandle<T> = Promise<T | null>;
export function openOverlay<T>(
  component: React.ComponentType<any>,
  props: Record<string, any>,
): OverlayHandle<T> {
  // Implementation uses a central OverlayManager and a promise resolver
}

// Full-screen modal contract
export interface FullScreenModalProps<T = any> {
  visible: boolean;
  title?: string;
  initialValue?: T;
  onConfirm?: (value: T) => void;
  onCancel?: () => void;
  dismissible?: boolean;
}

// Example: date picker full-screen
export interface DatePickerFullScreenProps {
  visible: boolean;
  initialDate?: string; // ISO 8601
  minDate?: string;
  maxDate?: string;
  onConfirm?: (dateIso: string) => void;
  onCancel?: () => void;
}

// NavStack simplified controller
export interface NavStackController {
  push(screenId: string, params?: Record<string, any>): void;
  pop(): void;
  replace(screenId: string, params?: Record<string, any>): void;
  openOverlay<T>(
    component: React.ComponentType<any>,
    props: any,
  ): Promise<T | null>;
}
```

Short-term component priorities

- High priority:
  - `OverlayManager` / `openOverlay()` core (promise-based overlay orchestration).
  - `FullScreenModal` base: backdrop, focus trap, enter/exit animation.
  - `ScreenScaffold` (standardize `FocusPageFrame` / titlebar / actions).
  - `InputWithPicker` (TextField that opens a full-screen picker and returns value).
  - `LogViewerModal` (for process detail and retained logs).
- Medium priority:
  - `BottomSheet` (quick actions), `SelectListFullScreen` (large lists), `ToastManager`.
- Low priority:
  - Advanced `FormFlow` / `StepFlow`, `ActionSheet`, `BlockingLoader`.

Potential future components

- `DatePickerFullScreen`, `TimePickerFullScreen` (if scheduling features are added)
- `FileBrowserFullScreen` (UI fallback for native pickers)
- `ConfirmDialog` / `DestructiveConfirmModal`
- `ListItemSelectable` / `VirtualizedSelectList` for very large data sets
- `ArtifactPreview` / `BuildReportExplorer`
- `NavBreadcrumbs` (for deep intra-screen navigation)

Required patterns and infra

- Central `OverlayManager` that orders overlays by priority and ensures hardware `Back` closes overlays first.
- Promise-based overlay opener to simplify sequential code and testing.
- Standard `ScreenScaffold` for all focus screens.
- Unit tests for `OverlayManager` and result-returning overlays.

Accessibility and motion guidance

- On open: move focus to the first interactive element; trap focus; mark `aria-modal` and announce the overlay.
- On close: restore focus to the originating control.
- Motion timings: push/pop 220ms, modal slide 180ms, backdrop fade 140ms.

Suggested implementation plan (next steps)

1. Implement `OverlayManager` + `FullScreenModal` with basic unit tests.
2. Refactor `PathPickerField` to call `openOverlay(FileBrowser)` as a fallback and keep `pickHostPath` as the desktop fast path.
3. Extract `LogViewerModal` and integrate into `ProcessDetailFocusScreen` via `openOverlay(LogViewer, { content })`.
4. Create `InputWithPicker` and migrate critical pickers (Unity executable picker, credentials composer).
5. Iterate: replace other pickers and confirmations with `openOverlay`-based overlays.

References (inspected files)

- apps/desktop/ui/src/App.tsx
- apps/desktop/ui/src/components/ProjectsFocusScreen.tsx
- apps/desktop/ui/src/components/ProjectWorkersFocusScreen.tsx
- apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx
- apps/desktop/ui/src/components/RepositoryProjectDetail.tsx
- apps/desktop/ui/src/components/CreateProjectWizard.tsx
- apps/desktop/ui/src/components/PathPickerField.tsx
- apps/desktop/ui/src/components/PublishDestinationsEditor.tsx

If you want, I can implement the `OverlayManager` and a basic `FullScreenModal` in `apps/desktop/ui/src/components` and migrate `PathPickerField` as a proof-of-concept. Should I start with that PoC?
